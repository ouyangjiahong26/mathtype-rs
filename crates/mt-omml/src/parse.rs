//! Reading Office Math into the equation tree.

use crate::dom::{self, Elem};
use crate::{
    ParseError, accent_is_under, accent_kind, class_of, delim_pair, fold_columns,
    font_of_properties, group_character_kind, side_of_name, space_of_text,
};
use mt_core::{
    Accent, AccentKind, BigOp, Char, Class, Fence, Font, Frac, FracKind, Limits, Math, MathStyle,
    Matrix, MatrixKind, Node, Row, Scripts, Side, Sqrt, Styled,
};

/// Reads an `<m:oMath>` or `<m:oMathPara>` fragment.
pub fn parse(xml: &str) -> Result<Math, ParseError> {
    let root = dom::document(xml)?;
    let body = match root.name.as_str() {
        "oMath" => sequence(&root)?,
        "oMathPara" => paragraph(&root)?,
        other => return Err(ParseError::Root(other.to_owned())),
    };
    Ok(Math {
        body,
        style: MathStyle::Full,
    })
}

/// The several `m:oMath` children of a paragraph as one body.
fn paragraph(e: &Elem) -> Result<Node, ParseError> {
    allowed(e, &["oMathParaPr", "oMath"])?;
    let mut items = Vec::new();
    for math in e.named("oMath") {
        flatten(&mut items, sequence(math)?);
    }
    Ok(sequenced(items))
}

/// A run of items, the shape every argument and the equation body share.
fn sequence(e: &Elem) -> Result<Node, ParseError> {
    let mut items = Vec::new();
    for child in content(e) {
        items.push(node_of(child)?);
    }
    Ok(sequenced(items))
}

/// The child elements of an argument, without the properties Office Math allows around them.
fn content(e: &Elem) -> Vec<&Elem> {
    e.elems()
        .filter(|child| child.name != "argPr" && child.name != "ctrlPr")
        .collect()
}

/// An empty row when there is nothing, the item itself when there is one, a row otherwise.
fn sequenced(items: Vec<Node>) -> Node {
    match items.len() {
        0 => Node::Row(Row::new()),
        1 => items.into_iter().next().expect("one item"),
        _ => Node::Row(items),
    }
}

/// Appends one node's items to a sequence, flattening a row.
fn flatten(items: &mut Vec<Node>, node: Node) {
    match node {
        Node::Row(row) => items.extend(row),
        other => items.push(other),
    }
}

/// An optional slot: Office Math spells "absent" as an empty argument.
fn optional(node: Node) -> Option<Node> {
    (!node.is_empty_slot()).then_some(node)
}

/// The one required argument called `name`.
fn required_argument(e: &Elem, name: &str) -> Result<Node, ParseError> {
    let child = e.child(name).ok_or_else(|| missing(e, name))?;
    sequence(child)
}

/// The argument called `name`, absent when it is missing or empty.
fn optional_argument(e: &Elem, name: &str) -> Result<Option<Node>, ParseError> {
    match e.child(name) {
        None => Ok(None),
        Some(child) => Ok(optional(sequence(child)?)),
    }
}

/// A property value: the `m:val` of a child of the property element `owner`.
fn property<'a>(e: &'a Elem, owner: &str, name: &str) -> Option<&'a str> {
    e.child(owner)?.child(name)?.attr("val")
}

/// A property value that has to be a single character.
fn character_property(
    e: &Elem,
    owner: &str,
    name: &str,
    default: char,
) -> Result<char, ParseError> {
    match property(e, owner, name) {
        None => Ok(default),
        Some(value) => {
            let mut chars = value.chars();
            match (chars.next(), chars.next()) {
                (Some(ch), None) => Ok(ch),
                _ => Err(invalid_property(owner, name, value)),
            }
        }
    }
}

/// Rejects any child element that does not belong to `names`.
fn allowed(e: &Elem, names: &[&str]) -> Result<(), ParseError> {
    for child in e.elems() {
        if !names.contains(&child.name.as_str()) {
            return Err(ParseError::UnknownElement(child.name.clone()));
        }
    }
    Ok(())
}

/// An error for a missing required child.
fn missing(e: &Elem, name: &str) -> ParseError {
    ParseError::Structure(format!("<{}> has no <{name}>", e.name))
}

/// An error for an unusable property value.
fn invalid_property(owner: &str, attribute: &str, value: &str) -> ParseError {
    ParseError::InvalidAttribute {
        element: owner.to_owned(),
        attribute: attribute.to_owned(),
        value: value.to_owned(),
    }
}

/// One element as a node.
fn node_of(e: &Elem) -> Result<Node, ParseError> {
    match e.name.as_str() {
        "r" => run(e),
        "f" => fraction(e),
        "rad" => radical(e),
        "sSub" | "sSup" | "sSubSup" | "sPre" => scripts(e),
        "d" => delimiter(e),
        "nary" => nary(e),
        "acc" => accent(e),
        "bar" => bar(e),
        "groupChr" => group_character(e),
        "limLow" => limit_low(e),
        "limUpp" => limit_upper(e),
        "func" => function(e),
        "m" => Ok(Node::Matrix(Box::new(table(e)?))),
        "eqArr" => equation_array(e),
        "box" | "phant" => {
            allowed(e, &["boxPr", "phantPr", "e"])?;
            Ok(Node::Styled(Box::new(Styled {
                body: required_argument(e, "e")?,
                style: None,
                font: None,
                class: None,
            })))
        }
        // A box with a border is an inner atom for spacing purposes; the drawn border has no field.
        "borderBox" => {
            allowed(e, &["borderBoxPr", "e"])?;
            Ok(Node::Styled(Box::new(Styled {
                body: required_argument(e, "e")?,
                style: None,
                font: None,
                class: Some(Class::Inner),
            })))
        }
        other => Err(ParseError::UnknownElement(other.to_owned())),
    }
}

/// `m:r`: a run of characters, literal text, a space or an alignment point.
fn run(e: &Elem) -> Result<Node, ParseError> {
    let properties = e.child("rPr");
    let normal = properties.is_some_and(|properties| properties.child_on("nor").is_some());
    let aligned = properties.is_some_and(|properties| properties.child_on("aln").is_some());
    let script = properties
        .and_then(|properties| properties.child("scr"))
        .and_then(|script| script.attr("val"));
    let style = properties
        .and_then(|properties| properties.child("sty"))
        .and_then(|style| style.attr("val"));
    let text: String = e.named("t").map(Elem::text).collect();
    // A single letter is a variable and is italic unless the properties say otherwise; anything
    // else is upright, which is how Office and MathML both read a bare run.
    let font = match font_of_properties(script, style, normal || !is_letter(&text)) {
        Some(font) => font,
        None => {
            let misstyled = style.is_some_and(|value| !matches!(value, "p" | "b" | "i" | "bi"));
            return Err(if misstyled {
                invalid_property("rPr", "sty", style.unwrap_or_default())
            } else {
                invalid_property("rPr", "scr", script.unwrap_or_default())
            });
        }
    };
    if aligned && text.trim().is_empty() {
        return Ok(Node::Align);
    }
    if normal {
        return Ok(text_node(text, font));
    }
    if let Some(space) = space_of_text(&text) {
        return Ok(Node::Space(space));
    }
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) => Ok(Node::Char(Char {
            ch,
            class: class_of(ch),
            font,
        })),
        _ => Ok(text_node(text, font)),
    }
}

/// Whether the run holds exactly one letter, the shape that is italic by default.
fn is_letter(text: &str) -> bool {
    let mut chars = text.chars();
    matches!((chars.next(), chars.next()), (Some(ch), None) if ch.is_alphabetic())
}

/// A literal text node.
fn text_node(text: String, font: Font) -> Node {
    Node::Text(Box::new(mt_core::TextRun { text, font }))
}

/// `m:f`: a fraction whose rule style comes from `m:type`.
fn fraction(e: &Elem) -> Result<Node, ParseError> {
    allowed(e, &["fPr", "num", "den"])?;
    let kind = match property(e, "fPr", "type") {
        None | Some("bar") => FracKind::Bar,
        Some("skw") => FracKind::Slash,
        Some("lin") => FracKind::Linear,
        Some("noBar") => FracKind::NoBar,
        Some(other) => return Err(invalid_property("fPr", "type", other)),
    };
    Ok(Node::Frac(Box::new(Frac {
        num: required_argument(e, "num")?,
        den: required_argument(e, "den")?,
        kind,
    })))
}

/// `m:rad`: a radical whose index `m:degHide` may suppress.
fn radical(e: &Elem) -> Result<Node, ParseError> {
    allowed(e, &["radPr", "deg", "e"])?;
    let hidden = e
        .child("radPr")
        .is_some_and(|properties| properties.child_on("degHide").is_some());
    let index = if hidden {
        None
    } else {
        optional_argument(e, "deg")?
    };
    Ok(Node::Sqrt(Box::new(Sqrt {
        index,
        radicand: required_argument(e, "e")?,
    })))
}

/// `m:sSub`, `m:sSup`, `m:sSubSup` and `m:sPre`: attached scripts.
fn scripts(e: &Elem) -> Result<Node, ParseError> {
    allowed(
        e,
        &["sSubPr", "sSupPr", "sSubSupPr", "sPrePr", "e", "sub", "sup"],
    )?;
    Ok(Node::Scripts(Box::new(Scripts {
        base: required_argument(e, "e")?,
        sub: optional_argument(e, "sub")?,
        sup: optional_argument(e, "sup")?,
    })))
}

/// `m:d`: delimiters around a body, or around a table.
fn delimiter(e: &Elem) -> Result<Node, ParseError> {
    allowed(e, &["dPr", "e"])?;
    let begin = delimiter_character(e, "begChr", '(')?;
    let end = delimiter_character(e, "endChr", ')')?;
    let arguments: Vec<&Elem> = e.named("e").collect();
    if arguments.is_empty() {
        return Err(missing(e, "e"));
    }
    // A table between delimiters is the same construct as a table that owns its delimiters.
    if let [only] = arguments.as_slice() {
        let children = content(only);
        if let [inner] = children.as_slice() {
            if matches!(inner.name.as_str(), "m" | "eqArr") {
                if let Node::Matrix(mut matrix) = node_of(inner)? {
                    matrix.delims = Some(delim_pair(begin, end));
                    return Ok(Node::Matrix(matrix));
                }
            }
        }
    }
    let mut items = Vec::new();
    for argument in arguments {
        flatten(&mut items, sequence(argument)?);
    }
    let (open, close) = delim_pair(begin, end);
    Ok(Node::Fence(Box::new(Fence {
        open,
        close,
        body: sequenced(items),
    })))
}

/// One delimiter character, where the empty string means "draw nothing".
fn delimiter_character(e: &Elem, name: &str, default: char) -> Result<Option<char>, ParseError> {
    match property(e, "dPr", name) {
        None => Ok(Some(default)),
        Some("") => Ok(None),
        Some(value) => {
            let mut chars = value.chars();
            match (chars.next(), chars.next()) {
                (Some(ch), None) => Ok(Some(ch)),
                _ => Err(invalid_property("dPr", name, value)),
            }
        }
    }
}

/// `m:nary`: a large operator with limits. The operand `m:e` is not part of the operator, so it
/// follows it in the sequence.
fn nary(e: &Elem) -> Result<Node, ParseError> {
    allowed(e, &["naryPr", "sub", "sup", "e"])?;
    let character = character_property(e, "naryPr", "chr", '\u{222B}')?;
    let limits = match property(e, "naryPr", "limLoc") {
        None | Some("subSup") => false,
        Some("undOvr") => true,
        Some(other) => return Err(invalid_property("naryPr", "limLoc", other)),
    };
    let operator = Node::Big(Box::new(BigOp {
        base: Node::Char(Char {
            ch: character,
            class: class_of(character),
            font: Font::Upright,
        }),
        sub: optional_argument(e, "sub")?,
        sup: optional_argument(e, "sup")?,
        limits,
    }));
    match optional_argument(e, "e")? {
        None => Ok(operator),
        Some(operand) => Ok(Node::Row(vec![operator, operand])),
    }
}

/// `m:acc`: an accent drawn with a character.
fn accent(e: &Elem) -> Result<Node, ParseError> {
    allowed(e, &["accPr", "e"])?;
    let character = character_property(e, "accPr", "chr", '\u{0302}')?;
    let under = position(e, "accPr", character)?;
    let base = required_argument(e, "e")?;
    Ok(match accent_kind(character) {
        Some(kind) => Node::Accent(Box::new(Accent {
            kind: orient(kind, under),
            base,
            under,
        })),
        // A character the model has no accent for is still over or under material; keeping it as
        // limits loses the accent spacing but not the mark.
        None => {
            let mark = Node::Char(Char {
                ch: character,
                class: class_of(character),
                font: Font::Upright,
            });
            if under {
                Node::Limits(Box::new(Limits {
                    base,
                    under: Some(mark),
                    over: None,
                    align: Side::Center,
                }))
            } else {
                Node::Limits(Box::new(Limits {
                    base,
                    under: None,
                    over: Some(mark),
                    align: Side::Center,
                }))
            }
        }
    })
}

/// `m:bar`: a rule over or under the base.
fn bar(e: &Elem) -> Result<Node, ParseError> {
    allowed(e, &["barPr", "e"])?;
    let under = match property(e, "barPr", "pos") {
        None | Some("top") => false,
        Some("bot") => true,
        Some(other) => return Err(invalid_property("barPr", "pos", other)),
    };
    Ok(Node::Accent(Box::new(Accent {
        kind: AccentKind::Bar,
        base: required_argument(e, "e")?,
        under,
    })))
}

/// `m:groupChr`: a brace, bracket or parenthesis stretched over or under the base.
fn group_character(e: &Elem) -> Result<Node, ParseError> {
    allowed(e, &["groupChrPr", "e"])?;
    let character = character_property(e, "groupChrPr", "chr", '{')?;
    let under = position(e, "groupChrPr", character)?;
    // `m:groupChr` is the schema's way to draw a mark above or below the base, so any accent
    // character can ride in it: `write` uses it for a mark drawn under the base, which `m:acc`
    // cannot say. A brace-like character keeps its own kind.
    let kind = group_character_kind(character)
        .or_else(|| accent_kind(character))
        .unwrap_or(AccentKind::OverBrace);
    Ok(Node::Accent(Box::new(Accent {
        kind: orient(kind, under),
        base: required_argument(e, "e")?,
        under,
    })))
}

/// Whether the mark is drawn below the base: `m:pos`, or the character's own side.
fn position(e: &Elem, owner: &str, character: char) -> Result<bool, ParseError> {
    Ok(match property(e, owner, "pos") {
        None => accent_is_under(character),
        Some("top") => false,
        Some("bot") => true,
        Some(other) => return Err(invalid_property(owner, "pos", other)),
    })
}

/// A brace-like accent on the side it is drawn: the position decides which one it is.
fn orient(kind: AccentKind, under: bool) -> AccentKind {
    match (kind, under) {
        (AccentKind::OverBrace, true) | (AccentKind::UnderBrace, true) => AccentKind::UnderBrace,
        (AccentKind::OverBracket, true) | (AccentKind::UnderBracket, true) => {
            AccentKind::UnderBracket
        }
        (AccentKind::OverParen, true) | (AccentKind::UnderParen, true) => AccentKind::UnderParen,
        (AccentKind::OverBrace, false) | (AccentKind::UnderBrace, false) => AccentKind::OverBrace,
        (AccentKind::OverBracket, false) | (AccentKind::UnderBracket, false) => {
            AccentKind::OverBracket
        }
        (AccentKind::OverParen, false) | (AccentKind::UnderParen, false) => AccentKind::OverParen,
        (other, _) => other,
    }
}

/// `m:limLow`: material below the base, possibly under an `m:limUpp` that carries material above.
fn limit_low(e: &Elem) -> Result<Node, ParseError> {
    allowed(e, &["limLowPr", "e", "lim"])?;
    let under = optional_argument(e, "lim")?;
    let body = e.child("e").ok_or_else(|| missing(e, "e"))?;
    let children = content(body);
    if let [inner] = children.as_slice() {
        if inner.name == "limUpp" {
            if let Node::Limits(limits) = limit_upper(inner)? {
                return Ok(Node::Limits(Box::new(Limits {
                    base: limits.base,
                    under,
                    over: limits.over,
                    align: Side::Center,
                })));
            }
        }
    }
    Ok(Node::Limits(Box::new(Limits {
        base: sequence(body)?,
        under,
        over: None,
        align: Side::Center,
    })))
}

/// `m:limUpp`: material above the base.
fn limit_upper(e: &Elem) -> Result<Node, ParseError> {
    allowed(e, &["limUppPr", "e", "lim"])?;
    Ok(Node::Limits(Box::new(Limits {
        base: required_argument(e, "e")?,
        under: None,
        over: optional_argument(e, "lim")?,
        align: Side::Center,
    })))
}

/// `m:func`: a function name applied to an argument, which the model spells as a row.
fn function(e: &Elem) -> Result<Node, ParseError> {
    allowed(e, &["funcPr", "fName", "e"])?;
    Ok(Node::Row(vec![
        required_argument(e, "fName")?,
        required_argument(e, "e")?,
    ]))
}

/// `m:m`: a table of rows and cells.
fn table(e: &Elem) -> Result<Matrix, ParseError> {
    allowed(e, &["mPr", "mr"])?;
    let mut rows: Vec<Row> = Vec::new();
    for row_element in e.named("mr") {
        allowed(row_element, &["e"])?;
        let mut row = Row::new();
        for cell in row_element.named("e") {
            row.push(sequence(cell)?);
        }
        if row.is_empty() {
            return Err(ParseError::Structure("<mr> has no <e>".to_owned()));
        }
        rows.push(row);
    }
    let width = rows.iter().map(|row| row.len()).max().unwrap_or(0);
    let (align, columns) = fold_columns(column_alignment(e, width)?);
    Ok(Matrix {
        rows,
        delims: None,
        kind: MatrixKind::Matrix,
        align,
        columns,
        rules: false,
    })
}

/// The per-column justification `m:mcs` describes; `m:baseJc` and `m:plcHide` have no field.
///
/// `m:count` says how many columns a justification covers, but a document is free to claim an
/// arbitrarily large number: the run of columns is capped at `limit`, the table's own width, so
/// that a hostile count cannot ask for a huge allocation. A table's columns are exactly its
/// widest row, so nothing meaningful is lost by the cap.
fn column_alignment(e: &Elem, limit: usize) -> Result<Vec<Side>, ParseError> {
    let Some(matrix_properties) = e.child("mPr") else {
        return Ok(Vec::new());
    };
    allowed(
        matrix_properties,
        &[
            "baseJc", "plcHide", "rSpRule", "cGpRule", "rSp", "cSp", "cGp", "mcs", "ctrlPr",
        ],
    )?;
    let mut columns = Vec::new();
    for set in matrix_properties.named("mcs") {
        allowed(set, &["mc"])?;
        for column in set.named("mc") {
            let properties = column.child("mcPr");
            if let Some(properties) = properties {
                allowed(properties, &["count", "mcJc"])?;
            }
            let count = match properties
                .and_then(|properties| properties.child("count"))
                .and_then(|count| count.attr("val"))
            {
                None => 1,
                Some(value) => value
                    .parse::<usize>()
                    .map_err(|_| invalid_property("count", "val", value))?,
            };
            let side = match properties
                .and_then(|properties| properties.child("mcJc"))
                .and_then(|jc| jc.attr("val"))
            {
                None => Side::Center,
                Some(value) => {
                    side_of_name(value).ok_or_else(|| invalid_property("mcJc", "val", value))?
                }
            };
            for _ in 0..count.max(1) {
                if columns.len() >= limit {
                    break;
                }
                columns.push(side);
            }
        }
    }
    Ok(columns)
}

/// `m:eqArr`: stacked equations, which the model spells as a one-column aligned table.
fn equation_array(e: &Elem) -> Result<Node, ParseError> {
    allowed(e, &["eqArrPr", "e"])?;
    let mut rows = Vec::new();
    for argument in e.named("e") {
        rows.push(vec![sequence(argument)?]);
    }
    if rows.is_empty() {
        return Err(missing(e, "e"));
    }
    Ok(Node::Matrix(Box::new(Matrix {
        rows,
        delims: None,
        kind: MatrixKind::Aligned,
        align: Side::Center,
        columns: Vec::new(),
        rules: false,
    })))
}
