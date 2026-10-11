//! Serialisation of the equation tree back to LaTeX.
//!
//! The output is chosen so that re-parsing it yields a tree equal to the input after
//! [`mt_core::Node::normalize`]. In particular every macro the parser understands has a
//! canonical spelling here.

use mt_core::{
    Accent, AccentKind, BigOp, Class, Delim, Fence, Font, Frac, FracKind, Limits, MathStyle,
    Matrix, MatrixKind, Node, Side, Space, Styled, TextRun,
};

use crate::symbols;

/// Render a node to a LaTeX fragment.
pub(crate) fn render_to_string(node: &Node) -> String {
    let mut out = String::new();
    render(node, &mut out);
    out
}

fn render(node: &Node, out: &mut String) {
    match node {
        Node::Row(row) => {
            for item in row {
                render(item, out);
            }
        }
        Node::Char(ch) => render_char(ch.ch, out),
        Node::Text(text) => render_text(text, out),
        Node::Space(space) => push_delim(out, space_tex(*space)),
        Node::Frac(frac) => render_frac(frac, out),
        Node::Sqrt(sqrt) => {
            out.push_str("\\sqrt");
            if let Some(index) = &sqrt.index {
                out.push('[');
                render(index, out);
                out.push(']');
            }
            render_always(&sqrt.radicand, out);
        }
        Node::Scripts(scripts) => {
            render_braced(&scripts.base, out);
            if let Some(sub) = &scripts.sub {
                out.push('_');
                render_braced(sub, out);
            }
            if let Some(sup) = &scripts.sup {
                out.push('^');
                render_braced(sup, out);
            }
        }
        Node::Limits(limits) => render_limits(limits, out),
        Node::Fence(fence) => render_fence(fence, out),
        Node::Matrix(matrix) => render_matrix(matrix, out),
        Node::Accent(accent) => render_accent(accent, out),
        Node::Big(big) => render_big(big, out),
        Node::Styled(styled) => render_styled(styled, out),
        Node::Align => out.push_str(" & "),
        Node::Error(text) => out.push_str(text),
    }
}

/// Render `node`, wrapping it in braces when a bare rendering would not bind as one argument.
fn render_braced(node: &Node, out: &mut String) {
    if node.is_row() {
        out.push('{');
        render(node, out);
        out.push('}');
    } else {
        render(node, out);
    }
}

/// Render a template argument, always delimited so a control word cannot absorb it and its
/// shape (bare node versus row) is irrelevant. Re-parsing strips the braces back to the
/// canonical form of the argument.
fn render_always(node: &Node, out: &mut String) {
    out.push('{');
    render(node, out);
    out.push('}');
}

/// Append a control-word name, terminating it when the next character could extend it.
///
/// A trailing space is skipped by the parser, so it never changes the parsed tree.
fn push_macro(out: &mut String, name: &str) {
    out.push('\\');
    out.push_str(name);
    if name.ends_with(|c: char| c.is_ascii_alphabetic()) {
        out.push(' ');
    }
}

/// Append a delimiter, which may be a bare character or a control word.
fn push_delim(out: &mut String, text: &str) {
    out.push_str(text);
    if text.ends_with(|c: char| c.is_ascii_alphabetic()) {
        out.push(' ');
    }
}

fn render_char(ch: char, out: &mut String) {
    if let Some(name) = symbols::macro_for(ch) {
        push_macro(out, name);
    } else if matches!(ch, '\\' | '{' | '}' | '$' | '%' | '&' | '#' | '_' | '^' | '~') {
        out.push('\\');
        out.push(ch);
    } else {
        out.push(ch);
    }
}

fn render_text(text: &TextRun, out: &mut String) {
    let macro_name = match text.font {
        Font::Bold => "\\textbf",
        Font::Italic => "\\textit",
        Font::Monospace => "\\texttt",
        Font::SansSerif => "\\textsf",
        _ => "\\text",
    };
    out.push_str(macro_name);
    out.push('{');
    escape_text(&text.text, out);
    out.push('}');
}

/// Render `\operatorname{...}` / `\operatorname*{...}`.
fn render_operatorname(text: &TextRun, star: bool, out: &mut String) {
    out.push_str("\\operatorname");
    if star {
        out.push('*');
    }
    out.push('{');
    escape_text(&text.text, out);
    out.push('}');
}

/// Write text-mode content with the characters TeX reserves escaped.
fn escape_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\backslash"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '$' => out.push_str("\\$"),
            '%' => out.push_str("\\%"),
            '&' => out.push_str("\\&"),
            '#' => out.push_str("\\#"),
            '_' => out.push_str("\\_"),
            other => out.push(other),
        }
    }
}

fn render_frac(frac: &Frac, out: &mut String) {
    match frac.kind {
        FracKind::Bar => {
            out.push_str("\\frac");
        }
        FracKind::Linear => {
            out.push('{');
            render(&frac.num, out);
            out.push_str("}/{");
            render(&frac.den, out);
            out.push('}');
            return;
        }
        _ => {
            // No rule: the general form, which the parser reads back as a rule-less fraction.
            out.push_str("\\genfrac{}{}{}{}");
        }
    }
    render_always(&frac.num, out);
    render_always(&frac.den, out);
}

fn render_fence(fence: &Fence, out: &mut String) {
    if fence.open == Delim::Paren && fence.close == Delim::Paren {
        let inner = match &fence.body {
            Node::Row(row) if row.len() == 1 => &row[0],
            other => other,
        };
        if let Node::Frac(frac) = inner {
            if frac.kind == FracKind::NoBar {
                out.push_str("\\binom");
                render_always(&frac.num, out);
                render_always(&frac.den, out);
                return;
            }
        }
    }
    out.push_str("\\left");
    push_delim(out, &delim_open_tex(fence.open));
    render(&fence.body, out);
    out.push_str("\\right");
    push_delim(out, &delim_close_tex(fence.close));
}

fn render_limits(limits: &Limits, out: &mut String) {
    if limits.under.is_none() {
        if let Some(Node::Char(ch)) = limits.over.as_ref() {
            if ch.ch == '↔' {
                out.push_str("\\overleftrightarrow");
                render_always(&limits.base, out);
                return;
            }
        }
    }
    match (&limits.under, &limits.over) {
        (None, Some(over)) => {
            out.push_str("\\overset");
            render_always(over, out);
            render_always(&limits.base, out);
        }
        (Some(under), None) => {
            out.push_str("\\underset");
            render_always(under, out);
            render_always(&limits.base, out);
        }
        (Some(under), Some(over)) => {
            out.push_str("\\overset");
            render_always(over, out);
            out.push('{');
            out.push_str("\\underset");
            render_always(under, out);
            render_always(&limits.base, out);
            out.push('}');
        }
        (None, None) => render_always(&limits.base, out),
    }
}

fn render_accent(accent: &Accent, out: &mut String) {
    push_macro(out, accent_macro(accent.kind, accent.under));
    render_always(&accent.base, out);
}

fn accent_macro(kind: AccentKind, under: bool) -> &'static str {
    match kind {
        AccentKind::Hat => "hat",
        AccentKind::WideHat => "widehat",
        AccentKind::Tilde => "tilde",
        AccentKind::WideTilde => "widetilde",
        AccentKind::Bar => "bar",
        AccentKind::Vector => "vec",
        AccentKind::Dot => "dot",
        AccentKind::DoubleDot => "ddot",
        AccentKind::TripleDot => "dddot",
        AccentKind::Breve => "breve",
        AccentKind::Check => "check",
        AccentKind::Acute => "acute",
        AccentKind::Grave => "grave",
        AccentKind::Circle => "mathring",
        AccentKind::Line => {
            if under {
                "underline"
            } else {
                "overline"
            }
        }
        AccentKind::OverBrace => "overbrace",
        AccentKind::UnderBrace => "underbrace",
        AccentKind::ArrowRight => "overrightarrow",
        AccentKind::ArrowLeft => "overleftarrow",
        AccentKind::Strike => "not",
        AccentKind::OverBracket => "overbracket",
        AccentKind::UnderBracket => "underbracket",
        AccentKind::OverParen => "overparen",
        AccentKind::UnderParen => "underparen",
    }
}

fn render_big(big: &BigOp, out: &mut String) {
    let default_limits = match &big.base {
        Node::Char(ch) => {
            if let Some(name) = symbols::big_op_macro(ch.ch) {
                push_macro(out, name);
            } else {
                render_char(ch.ch, out);
            }
            symbols::big_op_limits(ch.ch).unwrap_or(true)
        }
        Node::Text(text) => {
            if let Some(limits) = symbols::named_op(&text.text) {
                push_macro(out, &text.text);
                limits
            } else {
                render_operatorname(text, big.limits, out);
                true
            }
        }
        other => {
            render_braced(other, out);
            true
        }
    };
    if big.limits != default_limits {
        out.push_str(if big.limits { "\\limits" } else { "\\nolimits" });
    }
    if let Some(sub) = &big.sub {
        out.push('_');
        render_braced(sub, out);
    }
    if let Some(sup) = &big.sup {
        out.push('^');
        render_braced(sup, out);
    }
}

fn render_styled(styled: &Styled, out: &mut String) {
    if let Some(font) = styled.font {
        out.push('\\');
        out.push_str(font_macro(font));
        out.push('{');
        render(&styled.body, out);
        out.push('}');
        return;
    }
    if styled.class == Some(Class::Op) {
        if let Node::Text(text) = &styled.body {
            render_operatorname(text, false, out);
            return;
        }
    }
    if let Some(style) = styled.style {
        if let Node::Frac(frac) = &styled.body {
            if frac.kind == FracKind::Bar {
                match style {
                    MathStyle::Full => {
                        out.push_str("\\dfrac");
                        render_always(&frac.num, out);
                        render_always(&frac.den, out);
                        return;
                    }
                    MathStyle::Text => {
                        out.push_str("\\tfrac");
                        render_always(&frac.num, out);
                        render_always(&frac.den, out);
                        return;
                    }
                    _ => {}
                }
            }
        }
        out.push('{');
        out.push_str(style_macro(style));
        out.push(' ');
        render(&styled.body, out);
        out.push('}');
        return;
    }
    render(&styled.body, out);
}

fn style_macro(style: MathStyle) -> &'static str {
    match style {
        MathStyle::Full => "\\displaystyle",
        MathStyle::Text => "\\textstyle",
        MathStyle::Script => "\\scriptstyle",
        MathStyle::ScriptScript => "\\scriptscriptstyle",
    }
}

fn font_macro(font: Font) -> &'static str {
    match font {
        Font::DoubleStruck => "mathbb",
        Font::Script => "mathcal",
        Font::BoldScript => "mathbfcal",
        Font::Fraktur => "mathfrak",
        Font::BoldFraktur => "mathbffrak",
        Font::Bold => "mathbf",
        Font::Italic => "mathit",
        Font::BoldItalic => "boldsymbol",
        Font::Upright => "mathrm",
        Font::SansSerif => "mathsf",
        Font::SansSerifBold => "mathsf",
        Font::SansSerifItalic => "mathsf",
        Font::SansSerifBoldItalic => "mathsf",
        Font::Monospace => "mathtt",
        Font::Symbol => "mathrm",
        Font::Extra => "mathrm",
    }
}

fn render_matrix(matrix: &Matrix, out: &mut String) {
    let (name, spec) = if matrix.kind == MatrixKind::Array {
        ("array", Some(column_spec(matrix)))
    } else {
        (environment_name(matrix), None)
    };
    out.push_str("\\begin{");
    out.push_str(name);
    out.push('}');
    if let Some(spec) = spec {
        out.push('{');
        out.push_str(&spec);
        out.push('}');
    }
    for (row_index, row) in matrix.rows.iter().enumerate() {
        if row_index > 0 {
            out.push_str(" \\\\ ");
        }
        for (cell_index, cell) in row.iter().enumerate() {
            if cell_index > 0 {
                out.push_str(" & ");
            }
            render(cell, out);
        }
    }
    out.push_str("\\end{");
    out.push_str(name);
    out.push('}');
}

fn environment_name(matrix: &Matrix) -> &'static str {
    match matrix.kind {
        MatrixKind::Matrix => match matrix.delims {
            Some((Delim::Paren, Delim::Paren)) => "pmatrix",
            Some((Delim::Bracket, Delim::Bracket)) => "bmatrix",
            Some((Delim::Brace, Delim::Brace)) => "Bmatrix",
            Some((Delim::Abs, Delim::Abs)) => "vmatrix",
            Some((Delim::Norm, Delim::Norm)) => "Vmatrix",
            _ => "matrix",
        },
        MatrixKind::Cases => "cases",
        MatrixKind::Aligned => "aligned",
        MatrixKind::Pile => "gathered",
        MatrixKind::Array | MatrixKind::LongDivision => "array",
    }
}

fn column_spec(matrix: &Matrix) -> String {
    if !matrix.columns.is_empty() {
        return matrix
            .columns
            .iter()
            .map(|side| match side {
                Side::Left => 'l',
                Side::Center => 'c',
                Side::Right => 'r',
            })
            .collect();
    }
    let count = matrix.col_count().max(1);
    let ch = match matrix.align {
        Side::Left => 'l',
        Side::Center => 'c',
        Side::Right => 'r',
    };
    std::iter::repeat_n(ch, count).collect()
}

fn delim_open_tex(delim: Delim) -> String {
    match delim {
        Delim::Custom(ch) => return custom_delim_tex(ch),
        Delim::Paren => "(",
        Delim::Bracket => "[",
        Delim::Brace => "\\{",
        Delim::Abs => "|",
        Delim::Norm => "\\|",
        Delim::Angle => "\\langle",
        Delim::Floor => "\\lfloor",
        Delim::Ceil => "\\lceil",
        Delim::None => ".",
        Delim::Arrow => "\\uparrow",
        Delim::DoubleBar => "\\|",
    }
    .to_owned()
}

fn delim_close_tex(delim: Delim) -> String {
    match delim {
        Delim::Custom(ch) => return custom_delim_tex(ch),
        Delim::Paren => ")",
        Delim::Bracket => "]",
        Delim::Brace => "\\}",
        Delim::Abs => "|",
        Delim::Norm => "\\|",
        Delim::Angle => "\\rangle",
        Delim::Floor => "\\rfloor",
        Delim::Ceil => "\\rceil",
        Delim::None => ".",
        Delim::Arrow => "\\downarrow",
        Delim::DoubleBar => "\\|",
    }
    .to_owned()
}

/// LaTeX for a custom delimiter character, preferring its macro spelling.
fn custom_delim_tex(ch: char) -> String {
    match symbols::macro_for(ch) {
        Some(name) => format!("\\{name}"),
        None => ch.to_string(),
    }
}

fn space_tex(space: Space) -> &'static str {
    match space {
        Space::Thin => "\\,",
        Space::Medium => "\\:",
        Space::Thick => "\\;",
        Space::Sixteenth => "\\ ",
        Space::Quad => "\\quad",
        Space::DoubleQuad => "\\qquad",
        Space::NegativeThin => "\\!",
        Space::NegativeMedium => "\\hspace{-0.2222em}",
        Space::NegativeThick => "\\hspace{-0.2778em}",
    }
}
