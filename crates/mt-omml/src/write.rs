//! Writing Office Math from the equation tree.

use crate::{
    accent_char, delim_chars, font_properties, is_group_character, side_name, space_text, NAMESPACE,
};
use mt_core::{Accent, AccentKind, Class, Delim, Font, FracKind, Math, Matrix, MatrixKind, Node, Scripts, Styled};
use quick_xml::escape::escape;

/// Writes an `<m:oMath>` fragment with the namespace declared, ready to paste into a paragraph.
pub fn write(math: &Math) -> String {
    let mut out = String::new();
    out.push_str("<m:oMath xmlns:m=\"");
    out.push_str(NAMESPACE);
    out.push_str("\">");
    items(&mut out, &math.body);
    out.push_str("</m:oMath>");
    out
}

/// Writes an `<m:oMathPara>` fragment, the display form of an equation.
///
/// `align` is the paragraph justification (`center`, `centerGroup`, `left` or `right`); anything
/// else, including `None`, is written as centred.
pub fn write_paragraph(math: &Math, align: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str("<m:oMathPara xmlns:m=\"");
    out.push_str(NAMESPACE);
    out.push_str("\">");
    if let Some(align) = align {
        out.push_str("<m:oMathParaPr>");
        valued(&mut out, "m:jc", justification(align));
        out.push_str("</m:oMathParaPr>");
    }
    out.push_str("<m:oMath>");
    items(&mut out, &math.body);
    out.push_str("</m:oMath></m:oMathPara>");
    out
}

/// The justification Office Math knows, falling back to centred.
fn justification(align: &str) -> &'static str {
    match align {
        "left" => "left",
        "right" => "right",
        "centerGroup" => "centerGroup",
        _ => "center",
    }
}

/// Writes a node as a sequence of items, which is how Office Math spells a row.
fn items(out: &mut String, node: &Node) {
    match node {
        Node::Row(row) => {
            for entry in row {
                item(out, entry);
            }
        }
        other => item(out, other),
    }
}

/// Writes one item.
fn item(out: &mut String, node: &Node) {
    match node {
        Node::Row(row) => {
            for entry in row {
                item(out, entry);
            }
        }
        Node::Char(character) => {
            let mut buffer = [0u8; 4];
            run(out, character.ch.encode_utf8(&mut buffer), character.font, false);
        }
        Node::Text(text) => run(out, &text.text, text.font, true),
        Node::Space(space) => {
            out.push_str("<m:r>");
            text_element(out, space_text(*space));
            out.push_str("</m:r>");
        }
        Node::Frac(fraction) => {
            out.push_str("<m:f>");
            let rule = match fraction.kind {
                FracKind::Bar => None,
                FracKind::Slash => Some("skw"),
                FracKind::NoBar => Some("noBar"),
                FracKind::Linear => Some("lin"),
            };
            if let Some(rule) = rule {
                out.push_str("<m:fPr>");
                valued(out, "m:type", rule);
                out.push_str("</m:fPr>");
            }
            argument(out, "m:num", &fraction.num);
            argument(out, "m:den", &fraction.den);
            out.push_str("</m:f>");
        }
        Node::Sqrt(radical) => {
            out.push_str("<m:rad><m:radPr>");
            valued(out, "m:degHide", if radical.index.is_some() { "0" } else { "1" });
            out.push_str("</m:radPr>");
            if let Some(index) = &radical.index {
                argument(out, "m:deg", index);
            }
            argument(out, "m:e", &radical.radicand);
            out.push_str("</m:rad>");
        }
        Node::Scripts(scripts) => scripts_element(out, scripts),
        Node::Limits(limits) => limits_element(out, &limits.base, limits.under.as_ref(), limits.over.as_ref()),
        Node::Big(big) => match &big.base {
            // A character base is the n-ary operator itself.
            Node::Char(character) => {
                let mut buffer = [0u8; 4];
                let operator = character.ch.encode_utf8(&mut buffer);
                out.push_str("<m:nary><m:naryPr>");
                valued(out, "m:chr", operator);
                if big.limits {
                    valued(out, "m:limLoc", "undOvr");
                }
                out.push_str("</m:naryPr>");
                argument_or_empty(out, "m:sub", big.sub.as_ref());
                argument_or_empty(out, "m:sup", big.sup.as_ref());
                argument_or_empty(out, "m:e", None);
                out.push_str("</m:nary>");
            }
            // Any other base cannot be an `m:chr`; write it as ordinary attached material.
            other => {
                if big.limits {
                    limits_element(out, other, big.sub.as_ref(), big.sup.as_ref());
                } else {
                    scripts_element(out, &Scripts { base: other.clone(), sub: big.sub.clone(), sup: big.sup.clone() });
                }
            }
        },
        Node::Fence(fence) => delimiter_element(out, fence.open, fence.close, &fence.body),
        Node::Matrix(matrix) => matrix_element(out, matrix),
        Node::Accent(accent) => accent_element(out, accent),
        Node::Styled(styled) => styled_element(out, styled),
        Node::Align => out.push_str("<m:r><m:rPr><m:aln m:val=\"1\"/></m:rPr><m:t></m:t></m:r>"),
        // Office Math has no error element, so the text is written literally.
        Node::Error(reason) => run(out, reason, Font::Upright, true),
    }
}

/// Writes a run: a character or literal text with the face it is set in.
fn run(out: &mut String, text: &str, font: Font, normal: bool) {
    out.push_str("<m:r>");
    if normal || font != Font::Italic {
        let (script, style) = font_properties(font);
        out.push_str("<m:rPr>");
        if normal {
            valued(out, "m:nor", "1");
        }
        if script != "roman" {
            valued(out, "m:scr", script);
        }
        valued(out, "m:sty", style);
        out.push_str("</m:rPr>");
    }
    text_element(out, text);
    out.push_str("</m:r>");
}

/// Writes attached scripts, choosing the element that records which slots are filled.
fn scripts_element(out: &mut String, scripts: &Scripts) {
    let name = match (&scripts.sub, &scripts.sup) {
        (Some(_), Some(_)) => "m:sSubSup",
        (None, Some(_)) => "m:sSup",
        _ => "m:sSub",
    };
    out.push('<');
    out.push_str(name);
    out.push('>');
    argument(out, "m:e", &scripts.base);
    if name != "m:sSup" {
        argument_or_empty(out, "m:sub", scripts.sub.as_ref());
    }
    if name != "m:sSub" {
        argument_or_empty(out, "m:sup", scripts.sup.as_ref());
    }
    close(out, name);
}

/// Writes over/under material, nesting `m:limUpp` inside `m:limLow` when both sides are filled.
fn limits_element(out: &mut String, base: &Node, under: Option<&Node>, over: Option<&Node>) {
    match (under, over) {
        (Some(under), Some(over)) => {
            out.push_str("<m:limLow><m:e><m:limUpp>");
            argument(out, "m:e", base);
            argument(out, "m:lim", over);
            out.push_str("</m:limUpp></m:e>");
            argument(out, "m:lim", under);
            out.push_str("</m:limLow>");
        }
        (Some(under), None) => {
            out.push_str("<m:limLow>");
            argument(out, "m:e", base);
            argument(out, "m:lim", under);
            out.push_str("</m:limLow>");
        }
        (None, Some(over)) => {
            out.push_str("<m:limUpp>");
            argument(out, "m:e", base);
            argument(out, "m:lim", over);
            out.push_str("</m:limUpp>");
        }
        (None, None) => {
            out.push_str("<m:limLow>");
            argument(out, "m:e", base);
            out.push_str("<m:lim/>");
            out.push_str("</m:limLow>");
        }
    }
}

/// Writes delimiters around a body.
fn delimiter_element(out: &mut String, open: Delim, close: Delim, body: &Node) {
    let (open_character, close_character) = delim_chars(open, close);
    out.push_str("<m:d><m:dPr>");
    valued(out, "m:begChr", &open_character.map(String::from).unwrap_or_default());
    valued(out, "m:endChr", &close_character.map(String::from).unwrap_or_default());
    out.push_str("</m:dPr>");
    argument(out, "m:e", body);
    out.push_str("</m:d>");
}

/// Writes a table, with delimiters around it when it has them.
fn matrix_element(out: &mut String, matrix: &Matrix) {
    let fenced = matrix.delims.is_some();
    if let Some((open, close)) = matrix.delims {
        let (open_character, close_character) = delim_chars(open, close);
        out.push_str("<m:d><m:dPr>");
        valued(out, "m:begChr", &open_character.map(String::from).unwrap_or_default());
        valued(out, "m:endChr", &close_character.map(String::from).unwrap_or_default());
        out.push_str("</m:dPr><m:e>");
    }
    // Stacked one-column material is an equation array; everything else is a matrix.
    if matrix.kind == MatrixKind::Aligned && matrix.col_count() <= 1 && !matrix.rows.is_empty() {
        out.push_str("<m:eqArr>");
        for row in &matrix.rows {
            out.push_str("<m:e>");
            for cell in row {
                items(out, cell);
            }
            out.push_str("</m:e>");
        }
        out.push_str("</m:eqArr>");
    } else {
        out.push_str("<m:m><m:mPr>");
        valued(out, "m:baseJc", "center");
        let columns = matrix.col_count();
        if columns > 0 {
            out.push_str("<m:mcs>");
            for column in 0..columns {
                let side = matrix.columns.get(column).copied().unwrap_or(matrix.align);
                out.push_str("<m:mc><m:mcPr>");
                valued(out, "m:count", "1");
                valued(out, "m:mcJc", side_name(side));
                out.push_str("</m:mcPr></m:mc>");
            }
            out.push_str("</m:mcs>");
        }
        out.push_str("</m:mPr>");
        for row in &matrix.rows {
            out.push_str("<m:mr>");
            // Office Math requires at least one cell per row.
            if row.is_empty() {
                out.push_str("<m:e/>");
            }
            for cell in row {
                argument(out, "m:e", cell);
            }
            out.push_str("</m:mr>");
        }
        out.push_str("</m:m>");
    }
    if fenced {
        out.push_str("</m:e></m:d>");
    }
}

/// Writes an accent as the element the shape calls for.
fn accent_element(out: &mut String, accent: &Accent) {
    let mut buffer = [0u8; 4];
    let mark = accent_char(accent.kind).encode_utf8(&mut buffer);
    if accent.kind == AccentKind::Bar {
        out.push_str("<m:bar><m:barPr>");
        valued(out, "m:pos", if accent.under { "bot" } else { "top" });
        out.push_str("</m:barPr>");
        argument(out, "m:e", &accent.base);
        out.push_str("</m:bar>");
    } else if is_group_character(accent.kind) {
        out.push_str("<m:groupChr><m:groupChrPr>");
        valued(out, "m:chr", mark);
        valued(out, "m:pos", if accent.under { "bot" } else { "top" });
        out.push_str("</m:groupChrPr>");
        argument(out, "m:e", &accent.base);
        out.push_str("</m:groupChr>");
    } else {
        out.push_str("<m:acc><m:accPr>");
        valued(out, "m:chr", mark);
        if accent.under {
            valued(out, "m:pos", "bot");
        }
        out.push_str("</m:accPr>");
        argument(out, "m:e", &accent.base);
        out.push_str("</m:acc>");
    }
}

/// Writes a wrapper: a border box only when the tree marks it as an inner atom.
fn styled_element(out: &mut String, styled: &Styled) {
    let bordered = styled.class == Some(Class::Inner) && styled.style.is_none() && styled.font.is_none();
    let name = if bordered { "m:borderBox" } else { "m:box" };
    out.push('<');
    out.push_str(name);
    out.push('>');
    argument(out, "m:e", &styled.body);
    close(out, name);
}

/// Writes one argument element around a node.
fn argument(out: &mut String, name: &str, node: &Node) {
    out.push('<');
    out.push_str(name);
    out.push('>');
    items(out, node);
    close(out, name);
}

/// Writes one argument element, empty when the slot is not filled.
fn argument_or_empty(out: &mut String, name: &str, node: Option<&Node>) {
    match node {
        Some(node) => argument(out, name, node),
        None => {
            out.push('<');
            out.push_str(name);
            out.push_str("/>");
        }
    }
}

/// Writes a text element, preserving leading and trailing whitespace.
fn text_element(out: &mut String, text: &str) {
    out.push_str("<m:t");
    if text.starts_with(char::is_whitespace) || text.ends_with(char::is_whitespace) {
        out.push_str(" xml:space=\"preserve\"");
    }
    out.push('>');
    out.push_str(&escape(text));
    out.push_str("</m:t>");
}

/// Writes an element whose only content is a `m:val` attribute.
fn valued(out: &mut String, name: &str, value: &str) {
    out.push('<');
    out.push_str(name);
    out.push_str(" m:val=\"");
    out.push_str(&escape(value));
    out.push_str("\"/>");
}

/// Writes a closing tag.
fn close(out: &mut String, name: &str) {
    out.push_str("</");
    out.push_str(name);
    out.push('>');
}
