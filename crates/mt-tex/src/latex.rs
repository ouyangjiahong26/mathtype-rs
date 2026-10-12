//! Serialisation of the equation tree back to LaTeX.
//!
//! The output is chosen so that re-parsing it yields a tree equal to the input after
//! [`mt_core::Node::normalize`]. In particular every macro the parser understands has a
//! canonical spelling here.
//!
//! # Canonical forms
//!
//! A few [`mt_core::Font`] faces have no standard LaTeX spelling. Rather than fold them onto an
//! unrelated macro (which would silently change the face on re-read) every face gets a distinct
//! canonical control word, [`Font::SansSerifBold`] as `\mathsfbf` and [`Font::Extra`] as
//! `\mathextra` for example; the parser accepts exactly the names this module writes. See
//! [`font_macro`].
//!
//! [`mt_core::Delim::Arrow`] is written as the `→` character, matching mt-mathml and mt-omml, and
//! is read back as `Delim::Arrow` rather than as a custom character.
//!
//! # Lossy exceptions
//!
//! A few nodes have no LaTeX spelling the parser reads back to the same tree. They are written
//! so the output at least parses, and the resulting difference is listed here:
//!
//! - [`mt_core::Node::Align`] is written as ` & ` only inside the cells of an `aligned`
//!   environment, where it reads back as an alignment point. Anywhere else it has no parseable
//!   spelling, so it is written as `\;` and reads back as a [`mt_core::Space::Thick`] space.
//! - [`mt_core::Node::Error`] is written as `\text{...}` with the text escaped, so the output
//!   parses; the fact that the markup was an error is lost on re-read.
//! - A fence with [`mt_core::Delim::Custom`] reads back as the delimiter the character spells
//!   when the parser treats that character specially: `Custom('{')` becomes
//!   [`mt_core::Delim::Brace`], `Custom('(')` becomes [`mt_core::Delim::Paren`], `Custom('|')`
//!   becomes [`mt_core::Delim::Abs`], `Custom('⟨')` becomes [`mt_core::Delim::Angle`], and so
//!   on. Custom characters the parser has no spelling for round-trip unchanged.
//! - A [`mt_core::Char`] whose class was forced to `Op` or `Inner` (classes with no `\math…`
//!   macro) is written without the class, which is lost on re-read. The six classes with
//!   macros (`\mathord`, `\mathbin`, `\mathrel`, `\mathopen`, `\mathclose`, `\mathpunct`) are
//!   written and round-trip.

use mt_core::{
    Accent, AccentKind, BigOp, Char, Class, Delim, Fence, Font, Frac, FracKind, Limits, MathStyle,
    Matrix, MatrixKind, Node, Side, Space, Styled, TextRun,
};

use crate::{parse::char_class, symbols};

/// Render a node to a LaTeX fragment.
pub(crate) fn render_to_string(node: &Node) -> String {
    let mut out = String::new();
    render(node, &mut out, false);
    out
}

/// `aligned` is true while rendering inside an `aligned` environment cell, the one context where
/// [`Node::Align`] has a parseable spelling.
fn render(node: &Node, out: &mut String, aligned: bool) {
    match node {
        Node::Row(row) => {
            for item in row {
                render(item, out, aligned);
            }
        }
        Node::Char(ch) => render_char_node(ch, out),
        Node::Text(text) => render_text(text, out),
        Node::Space(space) => push_delim(out, space_tex(*space)),
        Node::Frac(frac) => render_frac(frac, out, aligned),
        Node::Sqrt(sqrt) => {
            out.push_str("\\sqrt");
            if let Some(index) = &sqrt.index {
                out.push('[');
                render(index, out, aligned);
                out.push(']');
            }
            render_always(&sqrt.radicand, out, aligned);
        }
        Node::Scripts(scripts) => {
            render_braced(&scripts.base, out, aligned);
            if let Some(sub) = &scripts.sub {
                out.push('_');
                render_braced(sub, out, aligned);
            }
            if let Some(sup) = &scripts.sup {
                out.push('^');
                render_braced(sup, out, aligned);
            }
        }
        Node::Limits(limits) => render_limits(limits, out, aligned),
        Node::Fence(fence) => render_fence(fence, out, aligned),
        Node::Matrix(matrix) => render_matrix(matrix, out),
        Node::Accent(accent) => render_accent(accent, out, aligned),
        Node::Big(big) => render_big(big, out, aligned),
        Node::Styled(styled) => render_styled(styled, out, aligned),
        Node::Align => {
            if aligned {
                out.push_str(" & ");
            } else {
                // Outside an `aligned` cell there is no parseable spelling; see the module docs.
                out.push_str("\\;");
            }
        }
        Node::Error(text) => {
            // Written as escaped text so the output parses; see the module docs.
            out.push_str("\\text{");
            escape_text(text, out);
            out.push('}');
        }
    }
}

/// Render `node`, wrapping it in braces when a bare rendering would not bind as one argument.
///
/// A [`Node::Row`] renders as several atoms; [`Node::Scripts`] and [`Node::Big`] render as one
/// atom followed by their own scripts. Both must be delimited, otherwise `a^{b^c}` is written
/// `a^b^c` and rejected on re-read as a double superscript.
fn render_braced(node: &Node, out: &mut String, aligned: bool) {
    if node.is_row() || matches!(node, Node::Scripts(_) | Node::Big(_)) {
        out.push('{');
        render(node, out, aligned);
        out.push('}');
    } else {
        render(node, out, aligned);
    }
}

/// Render a template argument, always delimited so a control word cannot absorb it and its
/// shape (bare node versus row) is irrelevant. Re-parsing strips the braces back to the
/// canonical form of the argument.
fn render_always(node: &Node, out: &mut String, aligned: bool) {
    out.push('{');
    render(node, out, aligned);
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
    } else if ch == '\\' {
        // `\\` means a row separator, so a literal backslash needs its own control word.
        out.push_str("\\backslash");
    } else if matches!(ch, '{' | '}' | '$' | '%' | '&' | '#' | '_' | '^' | '~') {
        out.push('\\');
        out.push(ch);
    } else {
        out.push(ch);
    }
}

/// The class the parser gives the bare character before any explicit class macro.
fn natural_class(ch: char) -> Class {
    symbols::lookup_by_char(ch)
        .map(|(_, class)| class)
        .unwrap_or_else(|| char_class(ch))
}

/// Control word forcing an atom class; `Op` and `Inner` have none (see the module docs).
fn class_macro(class: Class) -> Option<&'static str> {
    Some(match class {
        Class::Ord => "mathord",
        Class::Bin => "mathbin",
        Class::Rel => "mathrel",
        Class::Open => "mathopen",
        Class::Close => "mathclose",
        Class::Punct => "mathpunct",
        _ => return None,
    })
}

/// Render a character, wrapping it in the canonical font and class macros when it differs from
/// what the parser would give the bare character. A character in its default font and class
/// writes directly; anything else is wrapped so the font and class survive a re-read.
fn render_char_node(ch: &Char, out: &mut String) {
    let natural = natural_class(ch.ch);
    let default_font = symbols::char_font(ch.ch);
    if ch.class == natural && ch.font == default_font {
        return render_char(ch.ch, out);
    }
    let mut depth = 0;
    if ch.class != natural {
        if let Some(name) = class_macro(ch.class) {
            push_macro(out, name);
            out.push('{');
            depth += 1;
        }
    }
    if ch.font != default_font {
        push_macro(out, font_macro(ch.font));
        out.push('{');
        depth += 1;
    }
    render_char(ch.ch, out);
    for _ in 0..depth {
        out.push('}');
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

fn render_frac(frac: &Frac, out: &mut String, aligned: bool) {
    match frac.kind {
        FracKind::Bar => {
            out.push_str("\\frac");
        }
        FracKind::Linear => {
            out.push('{');
            render(&frac.num, out, aligned);
            out.push_str("}/{");
            render(&frac.den, out, aligned);
            out.push('}');
            return;
        }
        _ => {
            // No rule: the general form, which the parser reads back as a rule-less fraction.
            out.push_str("\\genfrac{}{}{}{}");
        }
    }
    render_always(&frac.num, out, aligned);
    render_always(&frac.den, out, aligned);
}

fn render_fence(fence: &Fence, out: &mut String, aligned: bool) {
    if fence.open == Delim::Paren && fence.close == Delim::Paren {
        let inner = match &fence.body {
            Node::Row(row) if row.len() == 1 => &row[0],
            other => other,
        };
        if let Node::Frac(frac) = inner {
            if frac.kind == FracKind::NoBar {
                out.push_str("\\binom");
                render_always(&frac.num, out, aligned);
                render_always(&frac.den, out, aligned);
                return;
            }
        }
    }
    out.push_str("\\left");
    push_delim(out, &delim_open_tex(fence.open));
    render(&fence.body, out, aligned);
    out.push_str("\\right");
    push_delim(out, &delim_close_tex(fence.close));
}

fn render_limits(limits: &Limits, out: &mut String, aligned: bool) {
    if limits.under.is_none() {
        if let Some(Node::Char(ch)) = limits.over.as_ref() {
            if ch.ch == '↔' {
                out.push_str("\\overleftrightarrow");
                render_always(&limits.base, out, aligned);
                return;
            }
        }
    }
    match (&limits.under, &limits.over) {
        (None, Some(over)) => {
            out.push_str("\\overset");
            render_always(over, out, aligned);
            render_always(&limits.base, out, aligned);
        }
        (Some(under), None) => {
            out.push_str("\\underset");
            render_always(under, out, aligned);
            render_always(&limits.base, out, aligned);
        }
        (Some(under), Some(over)) => {
            out.push_str("\\overset");
            render_always(over, out, aligned);
            out.push('{');
            out.push_str("\\underset");
            render_always(under, out, aligned);
            render_always(&limits.base, out, aligned);
            out.push('}');
        }
        (None, None) => render_always(&limits.base, out, aligned),
    }
}

fn render_accent(accent: &Accent, out: &mut String, aligned: bool) {
    push_macro(out, accent_macro(accent.kind, accent.under));
    render_always(&accent.base, out, aligned);
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

fn render_big(big: &BigOp, out: &mut String, aligned: bool) {
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
            render_braced(other, out, aligned);
            true
        }
    };
    if big.limits != default_limits {
        out.push_str(if big.limits { "\\limits" } else { "\\nolimits" });
    }
    if let Some(sub) = &big.sub {
        out.push('_');
        render_braced(sub, out, aligned);
    }
    if let Some(sup) = &big.sup {
        out.push('^');
        render_braced(sup, out, aligned);
    }
}

fn render_styled(styled: &Styled, out: &mut String, aligned: bool) {
    // The class macro scopes the whole atom, so it wraps every other wrapper; `Op` and `Inner`
    // have no macro (module docs) and fall through.
    if let Some(name) = styled.class.and_then(class_macro) {
        push_macro(out, name);
        out.push('{');
        render_styled_body(styled, out, aligned);
        out.push('}');
        return;
    }
    render_styled_body(styled, out, aligned);
}

fn render_styled_body(styled: &Styled, out: &mut String, aligned: bool) {
    if let Some(font) = styled.font {
        out.push('\\');
        out.push_str(font_macro(font));
        out.push('{');
        render(&styled.body, out, aligned);
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
                        render_always(&frac.num, out, aligned);
                        render_always(&frac.den, out, aligned);
                        return;
                    }
                    MathStyle::Text => {
                        out.push_str("\\tfrac");
                        render_always(&frac.num, out, aligned);
                        render_always(&frac.den, out, aligned);
                        return;
                    }
                    _ => {}
                }
            }
        }
        out.push('{');
        out.push_str(style_macro(style));
        out.push(' ');
        render(&styled.body, out, aligned);
        out.push('}');
        return;
    }
    render(&styled.body, out, aligned);
}

fn style_macro(style: MathStyle) -> &'static str {
    match style {
        MathStyle::Full => "\\displaystyle",
        MathStyle::Text => "\\textstyle",
        MathStyle::Script => "\\scriptstyle",
        MathStyle::ScriptScript => "\\scriptscriptstyle",
    }
}

/// Canonical control word for a font, the inverse of `parse::font_macro`.
///
/// Names are the standard LaTeX macros where one exists and dedicated spellings elsewhere, so
/// that every [`Font`] round trips through LaTeX instead of being folded onto another face.
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
        Font::SansSerifBold => "mathsfbf",
        Font::SansSerifItalic => "mathsfit",
        Font::SansSerifBoldItalic => "mathsfbfit",
        Font::Monospace => "mathtt",
        Font::Symbol => "mathsymbol",
        Font::Extra => "mathextra",
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
    // Inside an `aligned` row a `&` spells an alignment point, the one context where
    // `Node::Align` has a parseable spelling.
    let aligned = matrix.kind == MatrixKind::Aligned;
    for (row_index, row) in matrix.rows.iter().enumerate() {
        if row_index > 0 {
            out.push_str(" \\\\ ");
        }
        for (cell_index, cell) in row.iter().enumerate() {
            if cell_index > 0 {
                out.push_str(" & ");
            }
            render(cell, out, aligned);
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
        Delim::Arrow => "→",
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
        Delim::Arrow => "→",
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
