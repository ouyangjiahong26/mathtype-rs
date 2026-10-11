//! Reading MathML into the equation tree.

use crate::dom::{self, Elem};
use crate::{
    accent_kind, class_of, class_of_marker, delim_pair, fence_class, fold_columns, font_of_marker, font_of_variant,
    is_large_op, kind_of_marker, side_of_name, space_of_width, ParseError,
};
use mt_core::{
    Accent, AccentKind, BigOp, Char, Class, Fence, Font, Frac, FracKind, Limits, Math, MathStyle, Matrix, MatrixKind,
    Node, Row, Scripts, Side, Sqrt, Styled, TextRun,
};

/// Reads a MathML presentation document.
pub fn parse(xml: &str) -> Result<Math, ParseError> {
    let root = dom::document(xml)?;
    if root.name != "math" {
        return Err(ParseError::Root(root.name));
    }
    let style = match root.attr("display") {
        Some("inline") => MathStyle::Text,
        _ => MathStyle::Full,
    };
    let mut items = elements(&root)?;
    let body = match items.len() {
        0 => Node::Row(Row::new()),
        1 => items.pop().expect("one item"),
        _ => Node::Row(items),
    };
    Ok(lift_style(body, style))
}

/// Moves a lone style wrapper up to the equation, which is how a preset above full size is
/// spelled in markup.
fn lift_style(body: Node, style: MathStyle) -> Math {
    match body {
        Node::Styled(styled) => {
            let Styled { body, style: inner, font, class } = *styled;
            match (inner, font, class) {
                (Some(preset), None, None) => Math { body, style: preset },
                (inner, font, class) => Math { body: Node::Styled(Box::new(Styled { body, style: inner, font, class })), style },
            }
        }
        other => Math { body: other, style },
    }
}

/// The child elements of `e` as a row.
fn elements(e: &Elem) -> Result<Vec<Node>, ParseError> {
    e.elems().map(node_of).collect()
}

/// The child elements of `e`.
fn child_elements(e: &Elem) -> Vec<&Elem> {
    e.elems().collect()
}

/// Exactly `arity` child elements, or an error naming the element.
fn fixed(e: &Elem, arity: usize) -> Result<Vec<&Elem>, ParseError> {
    let children = child_elements(e);
    if children.len() == arity {
        Ok(children)
    } else {
        Err(ParseError::Structure(format!(
            "<{}> takes {arity} child element(s), found {}",
            e.name,
            children.len()
        )))
    }
}

/// The single child of a one-argument element.
fn single(e: &Elem) -> Result<Node, ParseError> {
    node_of(fixed(e, 1)?[0])
}

/// The content of an argument slot: an empty row when there is none, the node itself when there
/// is one, and a row otherwise.
fn slot(e: &Elem) -> Result<Node, ParseError> {
    let children = child_elements(e);
    Ok(match children.len() {
        0 => Node::Row(Row::new()),
        1 => node_of(children[0])?,
        _ => Node::Row(children.into_iter().map(node_of).collect::<Result<Vec<_>, _>>()?),
    })
}

/// An optional slot: MathML spells "absent" as an empty row.
fn optional(node: Node) -> Option<Node> {
    (!node.is_empty_slot()).then_some(node)
}

/// Attribute markers carried in `class`, space separated.
fn markers(e: &Elem) -> impl Iterator<Item = &str> {
    e.attr("class").unwrap_or_default().split_whitespace()
}

/// The atom class override in `class`, if any.
fn class_attribute(e: &Elem) -> Option<Class> {
    markers(e).find_map(class_of_marker)
}

/// The font of a token element, from `mathvariant` or a face marker.
fn font_of(e: &Elem, default: Font) -> Result<Font, ParseError> {
    match markers(e).find_map(font_of_marker) {
        Some(font) => Ok(font),
        None => match e.attr("mathvariant") {
            None => Ok(default),
            Some(value) => font_of_variant(value).ok_or_else(|| invalid(e, "mathvariant", value)),
        },
    }
}

/// A literal text node.
fn text_node(text: String, font: Font) -> Node {
    Node::Text(Box::new(TextRun { text, font }))
}

/// The whole text of a token element as literal text.
fn literal(e: &Elem, font: Font) -> Node {
    text_node(e.text(), font)
}

/// An error for an attribute value the model cannot hold.
fn invalid(e: &Elem, attribute: &str, value: &str) -> ParseError {
    ParseError::InvalidAttribute { element: e.name.clone(), attribute: attribute.to_owned(), value: value.to_owned() }
}

/// One element as a node.
fn node_of(e: &Elem) -> Result<Node, ParseError> {
    match e.name.as_str() {
        "mrow" => Ok(Node::Row(elements(e)?)),
        "mi" => identifier(e),
        "mn" => number(e),
        "mo" => operator(e),
        "mtext" | "ms" => Ok(literal(e, font_of(e, Font::Upright)?)),
        "mspace" => space(e),
        "mfrac" => fraction(e),
        "msqrt" => Ok(Node::Sqrt(Box::new(Sqrt::square(single(e)?)))),
        "mroot" => radical(e),
        "msub" | "msup" => script(e, e.name == "msup"),
        "msubsup" => subscript_superscript(e),
        "munder" | "mover" | "munderover" => under_over(e),
        "mfenced" => fenced(e),
        "mtable" => Ok(Node::Matrix(Box::new(table(e)?))),
        "mstyle" => style(e),
        "mphantom" | "mpadded" => {
            Ok(Node::Styled(Box::new(Styled { body: slot(e)?, style: None, font: None, class: None })))
        }
        // An enclosure is an inner atom for spacing purposes; the drawn shape has no field.
        "menclose" => Ok(Node::Styled(Box::new(Styled { body: slot(e)?, style: None, font: None, class: Some(Class::Inner) }))),
        "malignmark" => Ok(Node::Align),
        "merror" => Ok(Node::Error(e.text())),
        other => Err(ParseError::UnknownElement(other.to_owned())),
    }
}

/// `mi`: a single character is a variable, anything longer is literal text.
fn identifier(e: &Elem) -> Result<Node, ParseError> {
    let text = e.text();
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) => {
            let default = if ch.is_alphabetic() { Font::Italic } else { Font::Upright };
            Ok(Node::Char(Char { ch, class: class_attribute(e).unwrap_or(Class::Ord), font: font_of(e, default)? }))
        }
        _ => Ok(text_node(text, font_of(e, Font::Upright)?)),
    }
}

/// `mn`: a single digit is a character, a longer run is one character per digit.
fn number(e: &Elem) -> Result<Node, ParseError> {
    let font = font_of(e, Font::Upright)?;
    let text = e.text();
    let chars: Vec<char> = text.chars().collect();
    Ok(match chars.as_slice() {
        [] => text_node(text, font),
        [ch] => Node::Char(Char { ch: *ch, class: Class::Ord, font }),
        _ => Node::Row(chars.into_iter().map(|ch| Node::Char(Char { ch, class: Class::Ord, font })).collect()),
    })
}

/// `mo`: class from the character, overridden by `fence`, `separators` or an explicit marker.
fn operator(e: &Elem) -> Result<Node, ParseError> {
    let font = font_of(e, Font::Upright)?;
    let text = e.text();
    let chars: Vec<char> = text.chars().collect();
    match chars.as_slice() {
        [ch] => {
            let mut class = class_of(*ch);
            if e.flag("fence") {
                if let Some(fenced) = fence_class(*ch) {
                    class = fenced;
                }
            }
            if e.flag("separators") {
                class = Class::Punct;
            }
            if let Some(marked) = class_attribute(e) {
                class = marked;
            }
            Ok(Node::Char(Char { ch: *ch, class, font }))
        }
        _ => Ok(text_node(text, font)),
    }
}

/// `mspace`: an explicit width on the em grid.
fn space(e: &Elem) -> Result<Node, ParseError> {
    let width = e.attr("width").unwrap_or("0");
    space_of_width(width).map(Node::Space).ok_or_else(|| invalid(e, "width", width))
}

/// `mfrac`: rule style from `bevelled` and `linethickness`.
fn fraction(e: &Elem) -> Result<Node, ParseError> {
    let children = fixed(e, 2)?;
    let bevelled = e.flag("bevelled");
    let no_rule = e.attr("linethickness").is_some_and(zero_length);
    let kind = match (bevelled, no_rule) {
        (true, true) => FracKind::Linear,
        (true, false) => FracKind::Slash,
        (false, true) => FracKind::NoBar,
        (false, false) => FracKind::Bar,
    };
    Ok(Node::Frac(Box::new(Frac { num: node_of(children[0])?, den: node_of(children[1])?, kind })))
}

/// Whether a length attribute spells zero.
fn zero_length(value: &str) -> bool {
    let number = value.trim().trim_end_matches(|ch: char| ch.is_ascii_alphabetic() || ch == '%');
    number.trim().parse::<f64>() == Ok(0.0)
}

/// `mroot`: an indexed radical, whose two children are the base followed by the index.
fn radical(e: &Elem) -> Result<Node, ParseError> {
    let children = fixed(e, 2)?;
    Ok(Node::Sqrt(Box::new(Sqrt { index: optional(node_of(children[1])?), radicand: node_of(children[0])? })))
}

/// `msub`/`msup`: one attached script.
fn script(e: &Elem, superscript: bool) -> Result<Node, ParseError> {
    let children = fixed(e, 2)?;
    let base = node_of(children[0])?;
    let script = optional(node_of(children[1])?);
    let (sub, sup) = if superscript { (None, script) } else { (script, None) };
    Ok(attached(base, sub, sup, false))
}

/// `msubsup`: both scripts.
fn subscript_superscript(e: &Elem) -> Result<Node, ParseError> {
    let children = fixed(e, 3)?;
    Ok(attached(node_of(children[0])?, optional(node_of(children[1])?), optional(node_of(children[2])?), false))
}

/// A scripted or limited base: a large operator with limits becomes [`Node::Big`].
fn attached(base: Node, sub: Option<Node>, sup: Option<Node>, limits: bool) -> Node {
    // A text base (`\lim` and friends) is only scripted as a large operator: the writer spells
    // `Node::Big` over a text base as these very elements, so it reads back as `Node::Big`.
    if large_operator(&base) || matches!(base, Node::Text(_)) {
        Node::Big(Box::new(BigOp { base, sub, sup, limits }))
    } else {
        Node::Scripts(Box::new(Scripts { base, sub, sup }))
    }
}

/// Whether the node is a large operator, which the tree spells as [`Node::Big`] once scripted.
fn large_operator(node: &Node) -> bool {
    matches!(node, Node::Char(Char { ch, class: Class::Op, .. }) if is_large_op(*ch))
}

/// `munder`/`mover`/`munderover`: an accent when the markup says so, otherwise limits.
fn under_over(e: &Elem) -> Result<Node, ParseError> {
    let children = fixed(e, if e.name == "munderover" { 3 } else { 2 })?;
    let base = node_of(children[0])?;
    let (under, over) = if e.name == "munder" {
        (optional(node_of(children[1])?), None)
    } else if e.name == "mover" {
        (None, optional(node_of(children[1])?))
    } else {
        (optional(node_of(children[1])?), optional(node_of(children[2])?))
    };
    let accented = if e.name == "munder" { e.flag("accentunder") } else { e.name == "mover" && e.flag("accent") };
    if accented {
        if let Some(kind) = under.as_ref().or(over.as_ref()).and_then(accent_script) {
            return Ok(Node::Accent(Box::new(Accent { kind, base, under: under.is_some() })));
        }
    }
    if large_operator(&base) {
        return Ok(Node::Big(Box::new(BigOp { base, sub: under, sup: over, limits: true })));
    }
    // A text base in an under/over element is a large operator (`\lim` and friends): the
    // writer spells `Node::Big` over a text base the same way, so it reads back as `Node::Big`.
    if matches!(base, Node::Text(_)) {
        return Ok(Node::Big(Box::new(BigOp { base, sub: under, sup: over, limits: true })));
    }
    let align = match e.attr("align") {
        None => Side::Center,
        Some(value) => side_of_name(value).ok_or_else(|| invalid(e, "align", value))?,
    };
    Ok(Node::Limits(Box::new(Limits { base, under, over, align })))
}

/// The accent an over/under script draws, when the script is a single character that names one.
fn accent_script(node: &Node) -> Option<AccentKind> {
    match node {
        Node::Char(Char { ch, .. }) => accent_kind(*ch),
        _ => None,
    }
}

/// `mfenced`: delimiters around a body, or around a table.
fn fenced(e: &Elem) -> Result<Node, ParseError> {
    let open = fence_character(e, "open", '(')?;
    let close = fence_character(e, "close", ')')?;
    let children = child_elements(e);
    if let [only] = children.as_slice() {
        if only.name == "mtable" {
            if let Node::Matrix(mut matrix) = node_of(only)? {
                matrix.delims = Some(delim_pair(open, close));
                return Ok(Node::Matrix(matrix));
            }
        }
    }
    let body = match children.len() {
        0 => Node::Row(Row::new()),
        1 => node_of(children[0])?,
        _ => Node::Row(children.into_iter().map(node_of).collect::<Result<Vec<_>, _>>()?),
    };
    let (open, close) = delim_pair(open, close);
    Ok(Node::Fence(Box::new(Fence { open, close, body })))
}

/// One delimiter character of `mfenced`, where the empty string means "draw nothing".
fn fence_character(e: &Elem, attribute: &str, default: char) -> Result<Option<char>, ParseError> {
    match e.attr(attribute) {
        None => Ok(Some(default)),
        Some("") => Ok(None),
        Some(value) => {
            let mut chars = value.chars();
            match (chars.next(), chars.next()) {
                (Some(ch), None) => Ok(Some(ch)),
                _ => Err(invalid(e, attribute, value)),
            }
        }
    }
}

/// `mtable`: rows of cells with column alignment and rules.
fn table(e: &Elem) -> Result<Matrix, ParseError> {
    let declared = match e.attr("columnalign") {
        None => Vec::new(),
        Some(value) => value
            .split_whitespace()
            .map(|name| side_of_name(name).ok_or_else(|| invalid(e, "columnalign", name)))
            .collect::<Result<Vec<_>, _>>()?,
    };
    let mut rows: Vec<Row> = Vec::new();
    let mut from_cells: Vec<Option<Side>> = Vec::new();
    for row_element in e.elems() {
        if row_element.name != "mtr" && row_element.name != "mlabeledtr" {
            return Err(ParseError::Structure(format!("<{}> cannot appear inside <mtable>", row_element.name)));
        }
        let mut row = Row::new();
        for cell in row_element.elems() {
            if cell.name != "mtd" {
                return Err(ParseError::Structure(format!("<{}> cannot appear inside <mtr>", cell.name)));
            }
            if let Some(value) = cell.attr("columnalign") {
                let side = side_of_name(value).ok_or_else(|| invalid(cell, "columnalign", value))?;
                if from_cells.len() <= row.len() {
                    from_cells.resize(row.len() + 1, None);
                }
                from_cells[row.len()] = Some(side);
            }
            row.push(slot(cell)?);
        }
        rows.push(row);
    }
    let width = rows.iter().map(Row::len).max().unwrap_or(0);
    let mut columns = Vec::new();
    for column in 0..width {
        match declared.get(column).or_else(|| from_cells.get(column).and_then(Option::as_ref)) {
            Some(side) => columns.push(*side),
            None => break,
        }
    }
    let (align, columns) = fold_columns(columns);
    let rules = e.attr("columnlines").is_some_and(|value| value.split_whitespace().any(|line| line == "solid"));
    let kind = markers(e).find_map(kind_of_marker).unwrap_or(MatrixKind::Matrix);
    Ok(Matrix { rows, delims: None, kind, align, columns, rules })
}

/// `mstyle`: a style, font or class wrapper.
fn style(e: &Elem) -> Result<Node, ParseError> {
    let body = slot(e)?;
    let style = cascade(e)?;
    let font = match e.attr("mathvariant") {
        None => markers(e).find_map(font_of_marker),
        Some(value) => Some(font_of_variant(value).ok_or_else(|| invalid(e, "mathvariant", value))?),
    };
    Ok(Node::Styled(Box::new(Styled { body, style, font, class: class_attribute(e) })))
}

/// The size preset `scriptlevel` and `displaystyle` name.
fn cascade(e: &Elem) -> Result<Option<MathStyle>, ParseError> {
    let Some(raw) = e.attr("scriptlevel") else {
        return Ok((e.attr("displaystyle") == Some("false")).then_some(MathStyle::Text));
    };
    let level = raw
        .strip_prefix('+')
        .unwrap_or(raw)
        .parse::<i32>()
        .map_err(|_| invalid(e, "scriptlevel", raw))?;
    Ok(Some(match level {
        i32::MIN..=0 => {
            if e.attr("displaystyle") == Some("false") {
                MathStyle::Text
            } else {
                MathStyle::Full
            }
        }
        1 => MathStyle::Script,
        _ => MathStyle::ScriptScript,
    }))
}
