//! Behavioural tests: structural assertions on the parsed tree, round-trip stability of the
//! serialiser, and the error paths.

use mt_core::{
    Accent, AccentKind, BigOp, Char, Class, Delim, Fence, Font, Frac, FracKind, Limits, Math,
    Matrix, MatrixKind, Node, Scripts, Side, Space, Sqrt, Styled, TextRun,
};

use crate::{parse, to_latex, to_latex_fragment};

fn v(ch: char) -> Node {
    Node::variable(ch)
}

fn sym(ch: char, class: Class) -> Node {
    Node::symbol(ch, class)
}

fn dig(digits: &str) -> Node {
    Node::row(digits.chars().map(|ch| sym(ch, Class::Ord)))
}

fn ro(items: Vec<Node>) -> Node {
    Node::row(items)
}

fn frac(numerator: Node, denominator: Node, kind: FracKind) -> Node {
    Node::Frac(Box::new(Frac {
        num: numerator,
        den: denominator,
        kind,
    }))
}

fn body(input: &str) -> Node {
    let mut math = parse(input).unwrap_or_else(|error| panic!("`{input}` failed: {error}"));
    math.body.normalize();
    math.body
}

/// Assert that `input` parses to the normalised `expected` tree.
fn check(input: &str, expected: Node) {
    let mut expected = expected;
    expected.normalize();
    assert_eq!(body(input), expected, "structure mismatch for `{input}`");
}

/// Serialise `node` (as the sole item of a row), re-parse, and assert equality; return the LaTeX.
fn assert_round_trips(node: Node) -> String {
    let math = Math::new(ro(vec![node]));
    let latex = to_latex(&math);
    let reparsed =
        parse(&latex).unwrap_or_else(|error| panic!("`{latex}` failed to re-parse: {error}"));
    let mut left = math.body.clone();
    let mut right = reparsed.body;
    left.normalize();
    right.normalize();
    assert_eq!(left, right, "round trip mismatch for `{latex}`");
    latex
}

// ------------------------------------------------------------------ structure

#[test]
fn single_variable() {
    check("a", ro(vec![v('a')]));
}

#[test]
fn binary_operator_classes() {
    check("a+b", ro(vec![v('a'), sym('+', Class::Bin), v('b')]));
    check("a=b", ro(vec![v('a'), sym('=', Class::Rel), v('b')]));
    check(
        "a(b)",
        ro(vec![
            v('a'),
            sym('(', Class::Open),
            v('b'),
            sym(')', Class::Close),
        ]),
    );
}

#[test]
fn superscript_only() {
    check(
        "x^2",
        ro(vec![Node::Scripts(Box::new(Scripts {
            base: v('x'),
            sub: None,
            sup: Some(sym('2', Class::Ord)),
        }))]),
    );
}

#[test]
fn subscript_and_superscript() {
    check(
        "x^a_b",
        ro(vec![Node::Scripts(Box::new(Scripts {
            base: v('x'),
            sub: Some(v('b')),
            sup: Some(v('a')),
        }))]),
    );
}

#[test]
fn multi_item_argument_stays_a_row() {
    check(
        "x^{a+b}",
        ro(vec![Node::Scripts(Box::new(Scripts {
            base: v('x'),
            sub: None,
            sup: Some(ro(vec![v('a'), sym('+', Class::Bin), v('b')])),
        }))]),
    );
}

#[test]
fn single_item_group_is_canonicalised_to_its_item() {
    check(
        "x^{2}",
        ro(vec![Node::Scripts(Box::new(Scripts {
            base: v('x'),
            sub: None,
            sup: Some(sym('2', Class::Ord)),
        }))]),
    );
}

#[test]
fn fraction_owns_two_slots() {
    check(
        "\\frac{a}{b}",
        ro(vec![frac(v('a'), v('b'), FracKind::Bar)]),
    );
}

#[test]
fn fraction_with_single_token_arguments() {
    check(
        "\\frac12",
        ro(vec![frac(
            sym('1', Class::Ord),
            sym('2', Class::Ord),
            FracKind::Bar,
        )]),
    );
}

#[test]
fn dfrac_is_a_display_styled_fraction() {
    check(
        "\\dfrac{a}{b}",
        ro(vec![Node::Styled(Box::new(Styled {
            body: frac(v('a'), v('b'), FracKind::Bar),
            style: Some(mt_core::MathStyle::Full),
            font: None,
            class: None,
        }))]),
    );
}

#[test]
fn over_splits_the_group() {
    check(
        "{a \\over b}",
        ro(vec![frac(v('a'), v('b'), FracKind::Bar)]),
    );
}

#[test]
fn square_root_slot() {
    check(
        "\\sqrt{x}",
        ro(vec![Node::Sqrt(Box::new(Sqrt {
            index: None,
            radicand: v('x'),
        }))]),
    );
}

#[test]
fn nth_root_index_slot() {
    check(
        "\\sqrt[3]{x}",
        ro(vec![Node::Sqrt(Box::new(Sqrt {
            index: Some(sym('3', Class::Ord)),
            radicand: v('x'),
        }))]),
    );
}

#[test]
fn sum_takes_limits() {
    check(
        "\\sum_{i=1}^n",
        ro(vec![Node::Big(Box::new(BigOp {
            base: sym('∑', Class::Op),
            sub: Some(ro(vec![v('i'), sym('=', Class::Rel), sym('1', Class::Ord)])),
            sup: Some(v('n')),
            limits: true,
        }))]),
    );
}

#[test]
fn integral_limits_are_sideways_by_default() {
    check(
        "\\int_0^\\infty",
        ro(vec![Node::Big(Box::new(BigOp {
            base: sym('∫', Class::Op),
            sub: Some(sym('0', Class::Ord)),
            sup: Some(sym('∞', Class::Ord)),
            limits: false,
        }))]),
    );
}

#[test]
fn explicit_limits_switches_placement() {
    check(
        "\\int\\limits_0^1",
        ro(vec![Node::Big(Box::new(BigOp {
            base: sym('∫', Class::Op),
            sub: Some(sym('0', Class::Ord)),
            sup: Some(sym('1', Class::Ord)),
            limits: true,
        }))]),
    );
    check(
        "\\sum\\nolimits_{i}",
        ro(vec![Node::Big(Box::new(BigOp {
            base: sym('∑', Class::Op),
            sub: Some(v('i')),
            sup: None,
            limits: false,
        }))]),
    );
}

#[test]
fn named_operator_gets_a_text_base() {
    check(
        "\\lim_{x\\to0}",
        ro(vec![Node::Big(Box::new(BigOp {
            base: Node::Text(Box::new(TextRun {
                text: "lim".to_owned(),
                font: Font::Upright,
            })),
            sub: Some(ro(vec![v('x'), sym('→', Class::Rel), sym('0', Class::Ord)])),
            sup: None,
            limits: true,
        }))]),
    );
}

#[test]
fn left_right_builds_a_fence() {
    check(
        "\\left(x\\right)",
        ro(vec![Node::Fence(Box::new(Fence {
            open: Delim::Paren,
            close: Delim::Paren,
            body: v('x'),
        }))]),
    );
}

#[test]
fn missing_delimiter_side_is_none() {
    check(
        "\\left.\\frac{a}{b}\\right|",
        ro(vec![Node::Fence(Box::new(Fence {
            open: Delim::None,
            close: Delim::Abs,
            body: frac(v('a'), v('b'), FracKind::Bar),
        }))]),
    );
}

#[test]
fn bare_parentheses_are_plain_characters() {
    check(
        "(x)",
        ro(vec![sym('(', Class::Open), v('x'), sym(')', Class::Close)]),
    );
}

#[test]
fn big_sizing_macro_is_ignored() {
    check(
        "\\bigl(x\\bigr)",
        ro(vec![sym('(', Class::Open), v('x'), sym(')', Class::Close)]),
    );
}

#[test]
fn accents_over_and_under() {
    check(
        "\\hat{x}",
        ro(vec![Node::Accent(Box::new(Accent {
            kind: AccentKind::Hat,
            base: v('x'),
            under: false,
        }))]),
    );
    check(
        "\\vec{v}",
        ro(vec![Node::Accent(Box::new(Accent {
            kind: AccentKind::Vector,
            base: v('v'),
            under: false,
        }))]),
    );
    check(
        "\\dot{x}",
        ro(vec![Node::Accent(Box::new(Accent {
            kind: AccentKind::Dot,
            base: v('x'),
            under: false,
        }))]),
    );
    check(
        "\\underline{x}",
        ro(vec![Node::Accent(Box::new(Accent {
            kind: AccentKind::Line,
            base: v('x'),
            under: true,
        }))]),
    );
    check(
        "\\overline{x}",
        ro(vec![Node::Accent(Box::new(Accent {
            kind: AccentKind::Line,
            base: v('x'),
            under: false,
        }))]),
    );
    check(
        "\\overrightarrow{AB}",
        ro(vec![Node::Accent(Box::new(Accent {
            kind: AccentKind::ArrowRight,
            base: ro(vec![v('A'), v('B')]),
            under: false,
        }))]),
    );
}

#[test]
fn not_is_a_strike_accent() {
    check(
        "\\not=",
        ro(vec![Node::Accent(Box::new(Accent {
            kind: AccentKind::Strike,
            base: sym('=', Class::Rel),
            under: false,
        }))]),
    );
}

#[test]
fn overset_and_underset_are_limits() {
    check(
        "\\overset{a}{b}",
        ro(vec![Node::Limits(Box::new(Limits {
            base: v('b'),
            under: None,
            over: Some(v('a')),
            align: Side::Center,
        }))]),
    );
    check(
        "\\underset{a}{b}",
        ro(vec![Node::Limits(Box::new(Limits {
            base: v('b'),
            under: Some(v('a')),
            over: None,
            align: Side::Center,
        }))]),
    );
}

#[test]
fn font_macros_style_the_body() {
    check(
        "\\mathbb{R}",
        ro(vec![Node::Styled(Box::new(Styled {
            body: v('R'),
            style: None,
            font: Some(Font::DoubleStruck),
            class: None,
        }))]),
    );
    check(
        "\\mathbf{v}",
        ro(vec![Node::Styled(Box::new(Styled {
            body: v('v'),
            style: None,
            font: Some(Font::Bold),
            class: None,
        }))]),
    );
}

#[test]
fn text_macro_produces_a_text_run() {
    check(
        "\\text{abc}",
        ro(vec![Node::Text(Box::new(TextRun {
            text: "abc".to_owned(),
            font: Font::Upright,
        }))]),
    );
    check(
        "\\text{if }",
        ro(vec![Node::Text(Box::new(TextRun {
            text: "if ".to_owned(),
            font: Font::Upright,
        }))]),
    );
}

#[test]
fn operatorname_is_an_op_class_styled_node() {
    check(
        "\\operatorname{foo}",
        ro(vec![Node::Styled(Box::new(Styled {
            body: Node::Text(Box::new(TextRun {
                text: "foo".to_owned(),
                font: Font::Upright,
            })),
            style: None,
            font: None,
            class: Some(Class::Op),
        }))]),
    );
}

#[test]
fn operatorname_star_takes_limits() {
    check(
        "\\operatorname*{lim}_{x}",
        ro(vec![Node::Big(Box::new(BigOp {
            base: Node::Text(Box::new(TextRun {
                text: "lim".to_owned(),
                font: Font::Upright,
            })),
            sub: Some(v('x')),
            sup: None,
            limits: true,
        }))]),
    );
}

#[test]
fn binomial_is_a_delimited_rule_less_fraction() {
    check(
        "\\binom{n}{k}",
        ro(vec![Node::Fence(Box::new(Fence {
            open: Delim::Paren,
            close: Delim::Paren,
            body: frac(v('n'), v('k'), FracKind::NoBar),
        }))]),
    );
}

#[test]
fn style_switch_wraps_the_row() {
    check(
        "\\displaystyle \\frac12",
        ro(vec![Node::Styled(Box::new(Styled {
            body: frac(sym('1', Class::Ord), sym('2', Class::Ord), FracKind::Bar),
            style: Some(mt_core::MathStyle::Full),
            font: None,
            class: None,
        }))]),
    );
}

#[test]
fn spacing_macros_map_to_spaces() {
    check("\\,", ro(vec![Node::Space(Space::Thin)]));
    check("\\:", ro(vec![Node::Space(Space::Medium)]));
    check("\\;", ro(vec![Node::Space(Space::Thick)]));
    check("\\!", ro(vec![Node::Space(Space::NegativeThin)]));
    check("\\quad", ro(vec![Node::Space(Space::Quad)]));
    check("\\qquad", ro(vec![Node::Space(Space::DoubleQuad)]));
    check("\\ ", ro(vec![Node::Space(Space::Sixteenth)]));
    check("\\hspace{2em}", ro(vec![Node::Space(Space::DoubleQuad)]));
}

#[test]
fn escaped_braces_are_delimiter_characters() {
    check(
        "\\{x\\}",
        ro(vec![sym('{', Class::Open), v('x'), sym('}', Class::Close)]),
    );
}

#[test]
fn greek_and_symbols_carry_their_classes() {
    check(
        "\\alpha",
        ro(vec![Node::char_with('α', Class::Ord, Font::Italic)]),
    );
    check("\\Gamma", ro(vec![sym('Γ', Class::Ord)]));
    check("\\leq", ro(vec![sym('≤', Class::Rel)]));
    check("\\times", ro(vec![sym('×', Class::Bin)]));
    check("\\infty", ro(vec![sym('∞', Class::Ord)]));
}

#[test]
fn pmatrix_becomes_a_fenced_matrix() {
    check(
        "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}",
        ro(vec![Node::Matrix(Box::new(Matrix {
            rows: vec![vec![v('a'), v('b')], vec![v('c'), v('d')]],
            delims: Some((Delim::Paren, Delim::Paren)),
            kind: MatrixKind::Matrix,
            align: Side::Center,
            columns: Vec::new(),
            rules: false,
        }))]),
    );
}

#[test]
fn cases_use_a_left_brace() {
    check(
        "\\begin{cases} a & b \\end{cases}",
        ro(vec![Node::Matrix(Box::new(Matrix {
            rows: vec![vec![v('a'), v('b')]],
            delims: Some((Delim::Brace, Delim::None)),
            kind: MatrixKind::Cases,
            align: Side::Left,
            columns: vec![Side::Left, Side::Left],
            rules: false,
        }))]),
    );
}

#[test]
fn array_honours_the_column_spec() {
    check(
        "\\begin{array}{cc} a & b \\end{array}",
        ro(vec![Node::Matrix(Box::new(Matrix {
            rows: vec![vec![v('a'), v('b')]],
            delims: None,
            kind: MatrixKind::Array,
            align: Side::Center,
            columns: vec![Side::Center, Side::Center],
            rules: true,
        }))]),
    );
}

#[test]
fn aligned_marks_alignment_points() {
    check(
        "\\begin{aligned} a &= b \\end{aligned}",
        ro(vec![Node::Matrix(Box::new(Matrix {
            rows: vec![vec![ro(vec![
                v('a'),
                Node::Align,
                sym('=', Class::Rel),
                v('b'),
            ])]],
            delims: None,
            kind: MatrixKind::Aligned,
            align: Side::Center,
            columns: Vec::new(),
            rules: false,
        }))]),
    );
}

#[test]
fn gathered_is_a_pile() {
    check(
        "\\begin{gathered} a \\\\ b \\end{gathered}",
        ro(vec![Node::Matrix(Box::new(Matrix {
            rows: vec![vec![v('a')], vec![v('b')]],
            delims: None,
            kind: MatrixKind::Pile,
            align: Side::Center,
            columns: Vec::new(),
            rules: false,
        }))]),
    );
}

#[test]
fn substack_is_a_pile() {
    check(
        "\\substack{a \\\\ b}",
        ro(vec![Node::Matrix(Box::new(Matrix {
            rows: vec![vec![v('a')], vec![v('b')]],
            delims: None,
            kind: MatrixKind::Pile,
            align: Side::Center,
            columns: Vec::new(),
            rules: false,
        }))]),
    );
}

#[test]
fn wrappers_are_stripped() {
    assert_eq!(parse("$x$").unwrap(), parse("x").unwrap());
    assert_eq!(parse("$$x$$").unwrap(), parse("x").unwrap());
    assert_eq!(parse("\\(x\\)").unwrap(), parse("x").unwrap());
    assert_eq!(parse("\\[x\\]").unwrap(), parse("x").unwrap());
}

// ---------------------------------------------------- non-canonical tree shapes

#[test]
fn bare_char_fraction_arguments_are_braced() {
    let node = frac(v('a'), v('b'), FracKind::Bar);
    assert_eq!(assert_round_trips(node), "\\frac{a}{b}");
}

#[test]
fn bare_scripts_radicand_is_braced() {
    let radicand = Node::Scripts(Box::new(Scripts {
        base: v('x'),
        sub: Some(v('i')),
        sup: Some(sym('2', Class::Ord)),
    }));
    let node = Node::Sqrt(Box::new(Sqrt {
        index: None,
        radicand,
    }));
    assert_eq!(assert_round_trips(node), "\\sqrt{x_i^2}");
}

#[test]
fn bare_char_script_base_round_trips() {
    let node = Node::Scripts(Box::new(Scripts {
        base: v('x'),
        sub: Some(v('i')),
        sup: Some(sym('2', Class::Ord)),
    }));
    assert_eq!(assert_round_trips(node), "x_i^2");
}

#[test]
fn bare_char_matrix_cell_and_fence_body_round_trip() {
    let matrix = Node::Matrix(Box::new(Matrix {
        rows: vec![vec![v('a'), v('b')]],
        delims: Some((Delim::Paren, Delim::Paren)),
        kind: MatrixKind::Matrix,
        align: Side::Center,
        columns: Vec::new(),
        rules: false,
    }));
    assert_eq!(
        assert_round_trips(matrix),
        "\\begin{pmatrix}a & b\\end{pmatrix}"
    );

    let fence = Node::Fence(Box::new(Fence {
        open: Delim::Paren,
        close: Delim::Paren,
        body: v('x'),
    }));
    assert_eq!(assert_round_trips(fence), "\\left(x\\right)");
}

#[test]
fn bare_char_sqrt_index_round_trips() {
    let node = Node::Sqrt(Box::new(Sqrt {
        index: Some(sym('3', Class::Ord)),
        radicand: v('x'),
    }));
    assert_eq!(assert_round_trips(node), "\\sqrt[3]{x}");
}

#[test]
fn omml_codec_pipeline_output() {
    // A tree in the canonical form mt-omml produces: single-item slots are the bare item.
    let math = parse("\\frac{a}{b}+\\sqrt{x_i^2}").unwrap();
    assert!(
        matches!(math.body.slot(0), Some(Node::Frac(_))),
        "first term should be a fraction"
    );
    assert_eq!(to_latex(&math), "\\frac{a}{b}+\\sqrt{x_i^2}");
}

// -------------------------------------------------------- round-trip regressions

#[test]
fn nested_scripts_round_trip() {
    // A self-scripted argument must be braced, otherwise `a^{b^c}` serialises to `a^b^c`.
    assert_eq!(to_latex(&parse("a^{b^c}").unwrap()), "a^{b^c}");
    assert_eq!(to_latex(&parse("a_{b_c}").unwrap()), "a_{b_c}");

    let nested = Node::Scripts(Box::new(Scripts {
        base: v('a'),
        sub: None,
        sup: Some(Node::Scripts(Box::new(Scripts {
            base: v('b'),
            sub: None,
            sup: Some(v('c')),
        }))),
    }));
    assert_eq!(assert_round_trips(nested), "a^{b^c}");
}

#[test]
fn escaped_characters_round_trip() {
    for ch in ['%', '#', '&', '$', '_', '^', '~'] {
        let latex = assert_round_trips(sym(ch, Class::Ord));
        assert_eq!(latex, format!("\\{ch}"));
    }
}

#[test]
fn literal_backslash_round_trips() {
    // `\\` is a row separator, so a literal backslash is written `\backslash`.
    assert_eq!(assert_round_trips(sym('\\', Class::Ord)), "\\backslash");
}

#[test]
fn backslash_inside_a_matrix_cell_survives() {
    let matrix = Node::Matrix(Box::new(Matrix {
        rows: vec![vec![sym('\\', Class::Ord), v('a')]],
        delims: None,
        kind: MatrixKind::Matrix,
        align: Side::Center,
        columns: Vec::new(),
        rules: false,
    }));
    let latex = assert_round_trips(matrix);
    assert!(
        !latex.contains("\\\\"),
        "a literal backslash must not become a row separator: {latex}"
    );
}

#[test]
fn every_font_face_round_trips() {
    let faces = [
        Font::DoubleStruck,
        Font::Script,
        Font::BoldScript,
        Font::Fraktur,
        Font::BoldFraktur,
        Font::Bold,
        Font::Italic,
        Font::BoldItalic,
        Font::Upright,
        Font::SansSerif,
        Font::SansSerifBold,
        Font::SansSerifItalic,
        Font::SansSerifBoldItalic,
        Font::Monospace,
        Font::Symbol,
        Font::Extra,
    ];
    for font in faces {
        let node = Node::Styled(Box::new(Styled {
            body: v('A'),
            style: None,
            font: Some(font),
            class: None,
        }));
        assert_round_trips(node);
    }
}

#[test]
fn bold_script_font_round_trips() {
    let node = Node::Styled(Box::new(Styled {
        body: v('g'),
        style: None,
        font: Some(Font::BoldScript),
        class: None,
    }));
    assert_eq!(assert_round_trips(node), "\\mathbfcal{g}");
}

#[test]
fn arrow_delimiter_round_trips() {
    let fence = Node::Fence(Box::new(Fence {
        open: Delim::Arrow,
        close: Delim::Arrow,
        body: v('x'),
    }));
    assert_eq!(assert_round_trips(fence), "\\left→x\\right→");

    // Arrow and Angle are distinct even though the model draws them alike.
    let mixed = Node::Fence(Box::new(Fence {
        open: Delim::Arrow,
        close: Delim::Angle,
        body: v('x'),
    }));
    let latex = assert_round_trips(mixed);
    assert!(
        latex.starts_with("\\left→x\\right\\rangle"),
        "unexpected `{latex}`"
    );
}

// ---------------------------------------------------------------- round trips

const CORPUS: &[&str] = &[
    "x",
    "a+b-c",
    "x^2",
    "x^a_b",
    "x^{a+b}",
    "x_1^2",
    "a^{b^c}",
    "a_{b_c}",
    "x^{a^{b}}",
    "{}",
    "a^{}",
    "{}^a",
    "\\%",
    "\\#",
    "\\&",
    "\\$",
    "\\_",
    "\\^",
    "\\~",
    "\\backslash",
    "50\\%",
    "\\#1",
    "\\frac{a}{b}",
    "\\frac12",
    "\\frac{a+b}{c}",
    "\\dfrac{a}{b}",
    "\\tfrac{a}{b}",
    "\\cfrac{a}{b}",
    "{a \\over b}",
    "\\sqrt{x}",
    "\\sqrt[3]{x}",
    "\\sqrt[n+1]{x}",
    "\\sum_{i=1}^n",
    "\\int_0^\\infty",
    "\\lim_{x\\to0}",
    "\\int\\limits_0^1",
    "\\sum\\nolimits_{i}",
    "\\prod_{k=1}^{n}",
    "\\log_2 x",
    "\\left(x\\right)",
    "\\left[\\frac{a}{b}\\right]",
    "\\left\\{\\begin{matrix}a\\end{matrix}\\right\\}",
    "\\left.\\frac{a}{b}\\right|",
    "\\left\\langle x \\right\\rangle",
    "\\left→x\\right→",
    "\\left\\lfloor x \\right\\rfloor",
    "\\bigl(x\\bigr)",
    "(x)",
    "\\hat{x}",
    "\\widehat{abc}",
    "\\bar{x}",
    "\\overline{abc}",
    "\\underline{abc}",
    "\\vec{v}",
    "\\dot{x}",
    "\\ddot{x}",
    "\\dddot{x}",
    "\\tilde{x}",
    "\\widetilde{abc}",
    "\\check{x}",
    "\\breve{x}",
    "\\acute{x}",
    "\\grave{x}",
    "\\mathring{x}",
    "\\overbrace{a+b}",
    "\\underbrace{a+b}",
    "\\overrightarrow{AB}",
    "\\overleftarrow{AB}",
    "\\overleftrightarrow{AB}",
    "\\not=",
    "\\overset{a}{b}",
    "\\underset{a}{b}",
    "\\stackrel{a}{b}",
    "\\mathbb{R}",
    "\\mathcal{L}",
    "\\mathfrak{g}",
    "\\mathbfcal{F}",
    "\\mathbffrak{g}",
    "\\mathbf{v}",
    "\\mathit{x}",
    "\\mathrm{d}",
    "\\mathsf{A}",
    "\\mathsfbf{A}",
    "\\mathsfit{A}",
    "\\mathsfbfit{A}",
    "\\mathtt{x}",
    "\\mathsymbol{x}",
    "\\mathextra{x}",
    "\\boldsymbol{\\alpha}",
    "\\bm{v}",
    "\\operatorname{foo}",
    "\\operatorname*{lim}_x",
    "\\displaystyle \\frac12",
    "\\textstyle x",
    "\\scriptstyle x",
    "\\scriptscriptstyle x",
    "\\text{hello world}",
    "\\textrm{abc}",
    "\\textnormal{abc}",
    "\\textbf{abc}",
    "\\text{escaped \\{brace\\}}",
    "\\geq",
    "\\alpha\\beta\\Gamma\\Delta",
    "\\pm\\mp\\times\\div\\cdot",
    "\\leq\\geq\\neq\\approx\\equiv",
    "\\subset\\subseteq\\in\\notin\\ni",
    "\\forall\\exists\\emptyset\\angle\\perp",
    "\\to\\rightarrow\\leftarrow\\leftrightarrow\\Rightarrow",
    "\\mapsto\\uparrow\\downarrow\\ldots\\cdots\\vdots\\ddots",
    "\\aleph\\hbar\\ell\\Re\\Im\\prime\\degree",
    "\\sum\\prod\\coprod\\int\\iint\\iiint\\oint",
    "\\bigcup\\bigcap\\bigvee\\bigwedge",
    "\\lim\\max\\min\\sup\\inf\\det\\gcd",
    "\\log\\ln\\exp\\sin\\cos\\tan\\cot\\sec\\csc\\arcsin\\arccos\\arctan\\sinh\\cosh\\tanh",
    "\\binom{n}{k}",
    "\\genfrac{(}{)}{0pt}{}{a}{b}",
    "\\genfrac{}{}{}{}{a}{b}",
    "\\{x\\}",
    "\\|x\\|",
    "\\langle x \\rangle",
    "\\lfloor x \\rfloor",
    "\\lceil x \\rceil",
    "a\\,b\\:c\\;d\\!e",
    "\\quad x\\qquad y",
    "\\hspace{1em}",
    "\\[?\\]",
    "\\begin{matrix} a & b \\\\ c & d \\end{matrix}",
    "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}",
    "\\begin{bmatrix} a & b \\end{bmatrix}",
    "\\begin{Bmatrix} a \\end{Bmatrix}",
    "\\begin{vmatrix} a \\end{vmatrix}",
    "\\begin{Vmatrix} a \\end{Vmatrix}",
    "\\begin{cases} a & b \\\\ c & d \\end{cases}",
    "\\begin{array}{lcr} a & b & c \\end{array}",
    "\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}",
    "\\begin{gathered} a \\\\ b \\end{gathered}",
    "\\begin{smallmatrix} a & b \\end{smallmatrix}",
    "\\substack{a \\\\ b}",
    "\\frac{a}{b}+\\sqrt{x_i^2}",
    "\\sum_{F}\\alpha",
];

#[test]
fn corpus_round_trips() {
    for input in CORPUS {
        let math =
            parse(input).unwrap_or_else(|error| panic!("`{input}` failed to parse: {error}"));
        let latex = to_latex(&math);
        let reparsed = parse(&latex)
            .unwrap_or_else(|error| panic!("`{input}` -> `{latex}` failed to re-parse: {error}"));
        let mut left = math;
        let mut right = reparsed;
        left.body.normalize();
        right.body.normalize();
        assert_eq!(
            left, right,
            "round trip mismatch for `{input}` (serialised as `{latex}`)"
        );
    }
}

#[test]
fn corpus_is_large_enough() {
    assert!(
        CORPUS.len() >= 40,
        "corpus only has {} entries",
        CORPUS.len()
    );
}

#[test]
fn fragment_serialises_a_node() {
    let math = parse("\\alpha+\\beta").unwrap();
    let fragment = to_latex_fragment(&math.body);
    let mut expected = math.body.clone();
    expected.normalize();
    let mut actual = parse(&fragment).unwrap().body;
    actual.normalize();
    assert_eq!(actual, expected);
    assert!(
        fragment.contains("\\alpha") && fragment.contains("\\beta"),
        "fragment was `{fragment}`"
    );
}

#[test]
fn digit_run_parses_to_ordinary_characters() {
    assert_eq!(body("12"), dig("12"));
    assert_eq!(
        body("\\frac12"),
        ro(vec![frac(
            sym('1', Class::Ord),
            sym('2', Class::Ord),
            FracKind::Bar
        )])
    );
}

// --------------------------------------------------------------------- errors

#[test]
fn unbalanced_group_is_an_error() {
    let error = parse("{a").unwrap_err();
    assert!(
        error.message.contains("unclosed"),
        "unexpected message: {error}"
    );
}

#[test]
fn dangling_superscript_is_an_error() {
    let error = parse("x^").unwrap_err();
    assert!(
        error.message.contains("end of input"),
        "unexpected message: {error}"
    );
}

#[test]
fn unknown_macro_reports_its_offset() {
    let error = parse("a\\foo").unwrap_err();
    assert!(
        error.message.contains("\\foo"),
        "unexpected message: {error}"
    );
    assert_eq!(error.offset, 1);
}

#[test]
fn left_without_right_is_an_error() {
    let error = parse("\\left( x").unwrap_err();
    assert!(
        error.message.contains("\\right"),
        "unexpected message: {error}"
    );
}

#[test]
fn unknown_environment_is_an_error() {
    let error = parse("\\begin{foo} x \\end{foo}").unwrap_err();
    assert!(
        error.message.contains("unknown environment"),
        "unexpected message: {error}"
    );
}

#[test]
fn malformed_hspace_is_an_error() {
    assert!(parse("\\hspace{x}").is_err());
    assert!(parse("\\hspace{1furlong}").is_err());
    assert!(parse("\\hspace").is_err());
}

#[test]
fn mismatched_end_is_an_error() {
    let error = parse("\\begin{matrix} a \\end{pmatrix}").unwrap_err();
    assert!(
        error.message.contains("does not match"),
        "unexpected message: {error}"
    );
}

#[test]
fn newcommand_is_rejected() {
    assert!(parse("\\newcommand{\\x}{1}").is_err());
}

// ------------------------------------------------------------------ nesting limit

#[test]
fn deeply_nested_input_is_rejected_not_crashed() {
    for input in [
        "{".repeat(129),
        format!("{0}{1}{0}", "{".repeat(129), "x"),
        format!("{}x", "\\left(".repeat(129)),
        format!("\\frac{}{{b}}", "\\frac{a}".repeat(129)),
    ] {
        let error = match parse(&input) {
            Ok(_) => panic!(
                "deeply nested input parsed: {}",
                input.chars().take(24).collect::<String>()
            ),
            Err(error) => error,
        };
        assert!(
            error.message.contains("nests deeper"),
            "unexpected error for {}: {error}",
            input.chars().take(24).collect::<String>()
        );
    }
    // Just inside the limit the parser still reports the real syntax error, not the depth gate.
    let error = parse(&"{".repeat(64)).unwrap_err();
    assert!(
        error.message.contains("unclosed"),
        "unexpected message: {error}"
    );
}

// ------------------------------------------------------------------ font and class round trips

#[test]
fn forced_font_characters_keep_their_face() {
    // What the MathML reader produces for `<mi mathvariant="bold">a</mi>`: the face rides on
    // the character itself. Writing it wraps the character in the font macro, and re-parsing
    // keeps the face.
    for font in [
        Font::Bold,
        Font::DoubleStruck,
        Font::Monospace,
        Font::SansSerifItalic,
    ] {
        let node = Node::Char(Char {
            ch: 'a',
            class: Class::Ord,
            font,
        });
        let latex = to_latex_fragment(&node);
        assert!(
            latex.starts_with('\\'),
            "forced font `{font:?}` must be spelled with a macro, got `{latex}`"
        );
        let reparsed = parse(&latex).unwrap_or_else(|error| panic!("`{latex}` failed: {error}"));
        assert!(
            fragment_has_font(reparsed.body, font),
            "font `{font:?}` lost in `{latex}`"
        );
    }
    // Default face writes directly with no macro.
    assert_eq!(to_latex_fragment(&v('x')), "x");
}

/// Whether the node, or a single-item font wrapper inside it, carries `font`.
fn fragment_has_font(node: Node, font: Font) -> bool {
    match node {
        Node::Char(ch) => ch.font == font,
        Node::Styled(styled) => styled.font == Some(font) || fragment_has_font(styled.body, font),
        Node::Row(row) => row.into_iter().any(|item| fragment_has_font(item, font)),
        _ => false,
    }
}

#[test]
fn forced_class_characters_keep_their_class() {
    // What the MathML reader produces for `<mo class="mt-class-bin">x</mo>`.
    let node = Node::Char(Char {
        ch: 'x',
        class: Class::Bin,
        font: Font::Italic,
    });
    let latex = to_latex_fragment(&node);
    // `push_macro` writes a separating space after a control word; the parser skips it.
    assert_eq!(latex, "\\mathbin {x}");
    assert_eq!(
        body(&latex),
        ro(vec![Node::Char(Char {
            ch: 'x',
            class: Class::Bin,
            font: Font::Italic
        })])
    );
    // A forced class on a symbol written as a macro wraps the macro.
    let node = Node::Char(Char {
        ch: '→',
        class: Class::Ord,
        font: Font::Upright,
    });
    let latex = to_latex_fragment(&node);
    assert!(latex.starts_with("\\mathord"), "{latex}");
    assert_eq!(body(&latex), ro(vec![sym('→', Class::Ord)]));
    // Natural classes write directly.
    assert_eq!(to_latex_fragment(&sym('+', Class::Bin)), "+");
    // Class macros parse onto single characters and larger bodies alike.
    assert_eq!(
        body("\\mathrel{ab}"),
        ro(vec![Node::Styled(Box::new(Styled {
            body: ro(vec![v('a'), v('b')]),
            style: None,
            font: None,
            class: Some(Class::Rel),
        }))])
    );
}

#[test]
fn styled_bodies_keep_their_class() {
    // `\mathbin{\mathbf{x}}`: the class macro must scope the font wrapper, not be dropped by it.
    assert_round_trips(Node::Styled(Box::new(Styled {
        body: Node::Styled(Box::new(Styled {
            body: v('x'),
            style: None,
            font: Some(Font::Bold),
            class: None,
        })),
        style: None,
        font: None,
        class: Some(Class::Bin),
    })));
    // `\mathbin{x^2}`: same for a template body.
    assert_round_trips(Node::Styled(Box::new(Styled {
        body: Node::Scripts(Box::new(Scripts {
            base: v('x'),
            sub: None,
            sup: Some(sym('2', Class::Ord)),
        })),
        style: None,
        font: None,
        class: Some(Class::Bin),
    })));
}

// ------------------------------------------------------------------ lossy output spells parseably

#[test]
fn align_writes_parseably_in_its_contexts() {
    // Top level: no parseable spelling for an alignment point, so a placeholder space.
    let latex = to_latex_fragment(&Node::Align);
    assert_eq!(latex, "\\;");
    assert_eq!(body(&latex), ro(vec![Node::Space(Space::Thick)]));
    // Inside an `aligned` cell the alignment point round trips.
    let matrix = Matrix {
        rows: vec![vec![ro(vec![v('a'), Node::Align, v('b')])]],
        delims: None,
        kind: MatrixKind::Aligned,
        align: Side::Center,
        columns: Vec::new(),
        rules: false,
    };
    assert_round_trips(Node::Matrix(Box::new(matrix)));
}

#[test]
fn error_nodes_write_escaped_text() {
    let node = Node::Error("a & b {c}".to_owned());
    let latex = to_latex_fragment(&node);
    assert_eq!(latex, "\\text{a \\& b \\{c\\}}");
    // The output parses; the error nature is dropped (documented exception).
    assert_eq!(
        body(&latex),
        ro(vec![Node::Text(Box::new(TextRun {
            text: "a & b {c}".to_owned(),
            font: Font::Upright,
        }))])
    );
}
