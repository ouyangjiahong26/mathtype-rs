//! Writing MathML from the equation tree.

use crate::{
    accent_char, class_marker, class_of, delim_chars, font_marker, kind_marker, side_name, space_width, variant_of,
    NAMESPACE,
};
use mt_core::{Char, Class, Font, FracKind, Math, MathStyle, Matrix, Node, Side, Styled};

/// Writes a MathML presentation document.
///
/// The result is a self-contained `m:math` fragment with the namespace declared, so it can be
/// embedded anywhere a MathML element is allowed.
pub fn write(math: &Math) -> String {
    let mut out = String::new();
    out.push_str("<m:math xmlns:m=\"");
    out.push_str(NAMESPACE);
    out.push_str("\" display=\"");
    out.push_str(if math.style == MathStyle::Full { "block" } else { "inline" });
    out.push_str("\">");
    match math.style {
        MathStyle::Script | MathStyle::ScriptScript => {
            out.push_str("<m:mstyle scriptlevel=\"");
            out.push_str(if math.style == MathStyle::Script { "1" } else { "2" });
            out.push_str("\">");
            node(&mut out, &math.body);
            close(&mut out, "m:mstyle");
        }
        MathStyle::Full | MathStyle::Text => node(&mut out, &math.body),
    }
    close(&mut out, "m:math");
    out
}

/// Writes one node.
fn node(out: &mut String, value: &Node) {
    match value {
        Node::Row(row) => {
            if row.is_empty() {
                return out.push_str("<m:mrow/>");
            }
            open(out, "m:mrow", &[]);
            for item in row {
                node(out, item);
            }
            close(out, "m:mrow");
        }
        Node::Char(character) => character_node(out, character),
        Node::Text(text) => text_node(out, "m:mtext", &text.text, text.font),
        Node::Space(space) => empty_element(out, "m:mspace", &[("width", space_width(*space))]),
        Node::Frac(fraction) => {
            let mut attrs: Vec<(&str, &str)> = Vec::new();
            match fraction.kind {
                FracKind::Bar => {}
                FracKind::Slash => attrs.push(("bevelled", "true")),
                FracKind::NoBar => attrs.push(("linethickness", "0")),
                FracKind::Linear => {
                    attrs.push(("bevelled", "true"));
                    attrs.push(("linethickness", "0"));
                }
            }
            open(out, "m:mfrac", &attrs);
            node(out, &fraction.num);
            node(out, &fraction.den);
            close(out, "m:mfrac");
        }
        Node::Sqrt(radical) => match &radical.index {
            None => {
                open(out, "m:msqrt", &[]);
                node(out, &radical.radicand);
                close(out, "m:msqrt");
            }
            Some(index) => {
                open(out, "m:mroot", &[]);
                node(out, index);
                node(out, &radical.radicand);
                close(out, "m:mroot");
            }
        },
        Node::Scripts(scripts) => {
            attached(out, &scripts.base, scripts.sub.as_ref(), scripts.sup.as_ref(), false, &[]);
        }
        Node::Limits(limits) => {
            let align = side_name(limits.align);
            let attrs: &[(&str, &str)] = if limits.align == Side::Center { &[] } else { &[("align", align)] };
            attached(out, &limits.base, limits.under.as_ref(), limits.over.as_ref(), true, attrs);
        }
        Node::Big(big) => {
            attached(out, &big.base, big.sub.as_ref(), big.sup.as_ref(), big.limits, &[]);
        }
        Node::Fence(fence) => {
            let (open_character, close_character) = delim_chars(fence.open, fence.close);
            let open_text = open_character.map(String::from).unwrap_or_default();
            let close_text = close_character.map(String::from).unwrap_or_default();
            open(out, "m:mfenced", &[("open", &open_text), ("close", &close_text)]);
            node(out, &fence.body);
            close(out, "m:mfenced");
        }
        Node::Matrix(matrix) => matrix_node(out, matrix),
        Node::Accent(accent) => {
            let (name, flag) = if accent.under { ("m:munder", "accentunder") } else { ("m:mover", "accent") };
            open(out, name, &[(flag, "true")]);
            node(out, &accent.base);
            character_node(out, &Char { ch: accent_char(accent.kind), class: Class::Ord, font: Font::Upright });
            close(out, name);
        }
        Node::Styled(styled) => styled_node(out, styled),
        Node::Align => out.push_str("<m:malignmark/>"),
        // The model keeps markup it could not read in an error node; a MathML reader can show it.
        Node::Error(reason) => {
            open(out, "m:merror", &[]);
            out.push_str(&quick_xml::escape::escape(reason.as_str()));
            close(out, "m:merror");
        }
    }
}

/// Writes a base with an over/under or sub/sup pair.
///
/// The element name records which of the two slots are filled, and a missing slot the element
/// still takes is written as an empty row, which reads back as absent.
fn attached(out: &mut String, base: &Node, lower: Option<&Node>, upper: Option<&Node>, limits: bool, attrs: &[(&str, &str)]) {
    let name = match (lower, upper) {
        (Some(_), Some(_)) if limits => "m:munderover",
        (None, Some(_)) if limits => "m:mover",
        (_, _) if limits => "m:munder",
        (Some(_), Some(_)) => "m:msubsup",
        (None, Some(_)) => "m:msup",
        _ => "m:msub",
    };
    open(out, name, attrs);
    node(out, base);
    if !matches!(name, "m:msup" | "m:mover") {
        match lower {
            Some(lower) => node(out, lower),
            None => out.push_str("<m:mrow/>"),
        }
    }
    if !matches!(name, "m:msub" | "m:munder") {
        match upper {
            Some(upper) => node(out, upper),
            None => out.push_str("<m:mrow/>"),
        }
    }
    close(out, name);
}

/// Writes a character, choosing the token element MathML renders it as.
fn character_node(out: &mut String, character: &Char) {
    let name = if character.class == Class::Ord && character.ch.is_alphabetic() {
        "m:mi"
    } else if character.class == Class::Ord && character.ch.is_numeric() {
        "m:mn"
    } else {
        "m:mo"
    };
    let mut marker_tokens: Vec<&str> = Vec::new();
    if let Some(marker) = font_marker(character.font) {
        marker_tokens.push(marker);
    }
    if character.class != class_of(character.ch) {
        marker_tokens.push(class_marker(character.class));
    }
    let markers = marker_tokens.join(" ");
    let default = if name == "m:mi" { Font::Italic } else { Font::Upright };
    let mut attrs: Vec<(&str, &str)> = Vec::new();
    if character.font != default {
        if let Some(variant) = variant_of(character.font) {
            attrs.push(("mathvariant", variant));
        }
    }
    if name == "m:mo" && matches!(character.class, Class::Open | Class::Close) {
        attrs.push(("fence", "true"));
    }
    if !markers.is_empty() {
        attrs.push(("class", markers.as_str()));
    }
    open(out, name, &attrs);
    let mut buffer = [0u8; 4];
    let encoded = character.ch.encode_utf8(&mut buffer);
    out.push_str(&quick_xml::escape::escape(&*encoded));
    close(out, name);
}

/// Writes literal text in a token element.
fn text_node(out: &mut String, name: &str, text: &str, font: Font) {
    let mut marker_tokens: Vec<&str> = Vec::new();
    if let Some(marker) = font_marker(font) {
        marker_tokens.push(marker);
    }
    let markers = marker_tokens.join(" ");
    let mut attrs: Vec<(&str, &str)> = Vec::new();
    if font != Font::Upright {
        if let Some(variant) = variant_of(font) {
            attrs.push(("mathvariant", variant));
        }
    }
    if !markers.is_empty() {
        attrs.push(("class", markers.as_str()));
    }
    open(out, name, &attrs);
    out.push_str(&quick_xml::escape::escape(text));
    close(out, name);
}

/// Writes a table, wrapped in delimiters when it has them.
fn matrix_node(out: &mut String, matrix: &Matrix) {
    let fenced = matrix.delims.is_some();
    if let Some((open_delim, close_delim)) = matrix.delims {
        let (open_character, close_character) = delim_chars(open_delim, close_delim);
        let open_text = open_character.map(String::from).unwrap_or_default();
        let close_text = close_character.map(String::from).unwrap_or_default();
        open(out, "m:mfenced", &[("open", &open_text), ("close", &close_text)]);
    }
    let columns = if matrix.columns.is_empty() {
        if matrix.align == Side::Center { String::new() } else { side_name(matrix.align).to_owned() }
    } else {
        matrix.columns.iter().map(|side| side_name(*side)).collect::<Vec<_>>().join(" ")
    };
    let kind = kind_marker(matrix.kind);
    let mut attrs: Vec<(&str, &str)> = Vec::new();
    if !columns.is_empty() {
        attrs.push(("columnalign", columns.as_str()));
    }
    if matrix.rules {
        attrs.push(("rowlines", "solid"));
        attrs.push(("columnlines", "solid"));
    }
    if let Some(marker) = kind {
        attrs.push(("class", marker));
    }
    if matrix.rows.is_empty() {
        empty_element(out, "m:mtable", &attrs);
    } else {
        open(out, "m:mtable", &attrs);
        for row in &matrix.rows {
            if row.is_empty() {
                out.push_str("<m:mtr/>");
                continue;
            }
            open(out, "m:mtr", &[]);
            for cell in row {
                open(out, "m:mtd", &[]);
                node(out, cell);
                close(out, "m:mtd");
            }
            close(out, "m:mtr");
        }
        close(out, "m:mtable");
    }
    if fenced {
        close(out, "m:mfenced");
    }
}

/// Writes a style, font or class wrapper.
fn styled_node(out: &mut String, styled: &Styled) {
    let mut marker_tokens: Vec<&str> = Vec::new();
    if let Some(marker) = styled.font.and_then(font_marker) {
        marker_tokens.push(marker);
    }
    if let Some(class) = styled.class {
        marker_tokens.push(class_marker(class));
    }
    let markers = marker_tokens.join(" ");
    let mut attrs: Vec<(&str, &str)> = Vec::new();
    match styled.style {
        Some(MathStyle::Full) => {
            attrs.push(("scriptlevel", "0"));
            attrs.push(("displaystyle", "true"));
        }
        Some(MathStyle::Text) => {
            attrs.push(("scriptlevel", "0"));
            attrs.push(("displaystyle", "false"));
        }
        Some(MathStyle::Script) => attrs.push(("scriptlevel", "1")),
        Some(MathStyle::ScriptScript) => attrs.push(("scriptlevel", "2")),
        None => {}
    }
    if let Some(variant) = styled.font.and_then(variant_of) {
        attrs.push(("mathvariant", variant));
    }
    if !markers.is_empty() {
        attrs.push(("class", markers.as_str()));
    }
    open(out, "m:mstyle", &attrs);
    node(out, &styled.body);
    close(out, "m:mstyle");
}

/// Writes an opening tag.
fn open(out: &mut String, name: &str, attrs: &[(&str, &str)]) {
    out.push('<');
    out.push_str(name);
    for (key, value) in attrs {
        attribute(out, key, value);
    }
    out.push('>');
}

/// Writes a self-closing tag.
fn empty_element(out: &mut String, name: &str, attrs: &[(&str, &str)]) {
    out.push('<');
    out.push_str(name);
    for (key, value) in attrs {
        attribute(out, key, value);
    }
    out.push_str("/>");
}

/// Writes a closing tag.
fn close(out: &mut String, name: &str) {
    out.push_str("</");
    out.push_str(name);
    out.push('>');
}

/// Writes one attribute with an escaped value.
fn attribute(out: &mut String, key: &str, value: &str) {
    out.push(' ');
    out.push_str(key);
    out.push_str("=\"");
    out.push_str(&quick_xml::escape::escape(value));
    out.push('"');
}
