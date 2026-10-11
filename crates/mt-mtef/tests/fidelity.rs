//! Byte-level fidelity of the written equation stream.
//!
//! Every equation in the corpus is written, read back and written again; the two byte streams
//! must agree exactly, which is the guarantee another tool can rely on when it re-saves a
//! stream this crate produced. Entries the format cannot spell exactly are marked as
//! documented exceptions (see the canonical-forms section of the crate documentation) and
//! assert the read-back the format *can* express instead of the original tree.

use mt_core::{
    Accent, AccentKind, BigOp, Char, Class, Delim, Fence, Font, Frac, FracKind, Limits, Math,
    Matrix, MatrixKind, Node, Scripts, Side, Space, Sqrt, Styled, TextRun,
};
use mt_mtef::{parse, parse_bare, write, write_bare};

/// One corpus entry: an equation and what reading its stream back is expected to give.
struct Case {
    label: &'static str,
    math: Math,
    /// The documented read-back, or `None` when the tree round trips exactly (up to the
    /// canonical forms every stream shares, cleared by [`canon`]).
    exception: Option<Node>,
}

fn case(label: &'static str, node: Node) -> Case {
    Case {
        label,
        math: Math::new(node),
        exception: None,
    }
}

fn lost(label: &'static str, node: Node, read_back: Node) -> Case {
    Case {
        label,
        math: Math::new(node),
        exception: Some(read_back),
    }
}

fn ch(ch: char, class: Class, font: Font) -> Node {
    Node::Char(Char { ch, class, font })
}

fn frac(num: Node, den: Node, kind: FracKind) -> Node {
    Node::Frac(Box::new(Frac { num, den, kind }))
}

fn fence(open: Delim, close: Delim, body: Node) -> Node {
    Node::Fence(Box::new(Fence { open, close, body }))
}

fn accent(kind: AccentKind, base: Node, under: bool) -> Node {
    Node::Accent(Box::new(Accent { kind, base, under }))
}

fn scripts(base: Node, sub: Option<Node>, sup: Option<Node>) -> Node {
    Node::Scripts(Box::new(Scripts { base, sub, sup }))
}

fn limits(base: Node, under: Option<Node>, over: Option<Node>) -> Node {
    Node::Limits(Box::new(Limits {
        base,
        under,
        over,
        align: Side::Center,
    }))
}

fn big(base: Node, sub: Option<Node>, sup: Option<Node>, with_limits: bool) -> Node {
    Node::Big(Box::new(BigOp {
        base,
        sub,
        sup,
        limits: with_limits,
    }))
}

fn table(
    rows: Vec<Vec<Node>>,
    delims: Option<(Delim, Delim)>,
    kind: MatrixKind,
    align: Side,
    rules: bool,
) -> Node {
    Node::Matrix(Box::new(Matrix {
        rows,
        delims,
        kind,
        align,
        columns: Vec::new(),
        rules,
    }))
}

fn styled(
    body: Node,
    style: Option<mt_core::MathStyle>,
    font: Option<Font>,
    class: Option<Class>,
) -> Node {
    Node::Styled(Box::new(Styled {
        body,
        style,
        font,
        class,
    }))
}

/// The tree with the canonical forms every read-back shares applied: the format aligns a whole
/// table in one step, so a matrix never reads back its per-column alignment, and norm and
/// double bar are spellings of the same template.
fn canon(node: &mut Node) {
    if let Node::Matrix(matrix) = node {
        matrix.columns.clear();
    }
    let delims: Option<(&mut Delim, &mut Delim)> = match node {
        Node::Matrix(matrix) => matrix.delims.as_mut().map(|(open, close)| (open, close)),
        Node::Fence(fence) => Some((&mut fence.open, &mut fence.close)),
        _ => None,
    };
    if let Some((open, close)) = delims {
        for delim in [open, close] {
            if *delim == Delim::Norm {
                *delim = Delim::DoubleBar;
            }
        }
    }
    for index in 0..node.slot_count() {
        if let Some(slot) = node.slot_mut(index) {
            canon(slot);
        }
    }
}

/// Assert both levels of fidelity for one entry: the read-back tree, and that writing the
/// read-back reproduces the first write byte for byte — as a bare stream and as a complete
/// stream.
fn check(case: &Case) {
    let Case {
        label,
        math,
        exception,
    } = case;
    let bare = write_bare(math);
    let read = parse_bare(&bare).unwrap_or_else(|error| panic!("`{label}`: {error}"));
    let rewritten = write_bare(&read);
    assert_eq!(
        rewritten, bare,
        "`{label}`: writing the read-back must reproduce the bytes"
    );

    let stream = write(math);
    let read_stream = parse(&stream).unwrap_or_else(|error| panic!("`{label}`: {error}"));
    assert_eq!(
        write(&read_stream),
        stream,
        "`{label}`: stream round trip must reproduce the bytes"
    );

    let mut expected = match exception {
        Some(read_back) => read_back.clone(),
        None => math.body.clone(),
    };
    match &mut expected {
        Node::Row(_) => {}
        other => *other = Node::row([std::mem::replace(other, Node::empty())]),
    }
    let mut actual = read.body;
    expected.normalize();
    actual.normalize();
    canon(&mut expected);
    canon(&mut actual);
    assert_eq!(actual, expected, "`{label}`: read-back tree");
    assert_eq!(read.style, math.style, "`{label}`: read-back style");
}

/// The corpus: LaTeX-parsed equations covering the constructs the format spells.
fn latex_cases() -> Vec<Case> {
    [
        "a",
        "x+y=z",
        "\\frac{a+b}{c-d}",
        "\\frac{\\frac{a}{b}}{\\frac{c}{d}}",
        "\\frac{1}{2}+\\frac{x}{y}",
        "\\sqrt{x+1}",
        "\\sqrt[3]{27}",
        "\\sqrt[n+1]{x^{2}}",
        "x^2",
        "a_i",
        "x_i^j",
        "x^{a+b}",
        "e^{-x^2/2}",
        "x^{y^{z^{w}}}",
        "a^{ }",
        "(a+b)\\cdot(c-d)",
        "\\left(\\frac{a}{b}\\right)^2",
        "\\left[x, y\\right]",
        "\\left\\{x : x>0\\right\\}",
        "\\left|x\\right|",
        "\\left\\|A\\right\\|",
        "\\left\\lfloor x \\right\\rfloor",
        "\\left\\lceil x \\right\\rceil",
        "\\langle x \\rangle",
        "\\left. x \\right)",
        "\\binom{n}{k}",
        "\\hat{x}\\bar{y}\\vec{v}",
        "\\dot{x}\\ddot{y}\\tilde{z}",
        "\\widehat{abc}\\widetilde{def}",
        "\\overline{xy}\\underline{xy}",
        "\\overbrace{xy}^{n}",
        "\\underbrace{xy}_{n}",
        "\\overset{!}{=}",
        "\\begin{matrix}a&b\\\\c&d\\end{matrix}",
        "\\begin{pmatrix}1&0\\\\0&1\\end{pmatrix}",
        "\\begin{bmatrix}a&b\\\\c&d\\end{bmatrix}",
        "\\begin{vmatrix}a&b\\\\c&d\\end{vmatrix}",
        "\\begin{Vmatrix}a&b\\\\c&d\\end{Vmatrix}",
        "\\begin{Bmatrix}a&b\\\\c&d\\end{Bmatrix}",
        "\\begin{cases}a&1\\\\b&2\\end{cases}",
        "\\sum_{i=1}^{n}i",
        "\\prod_{k=1}^{m}k",
        "\\int_0^1 x\\,dx",
        "\\iint_D f",
        "\\oint_C F\\cdot dl",
        "\\bigcup_{i} A_i",
        "\\bigcap_{i} B_i",
        "\\int_0^{2\\pi}\\left|x^2+1\\right|\\,dx",
        "\\alpha\\beta\\gamma\\delta\\epsilon",
        "\\Gamma\\Delta\\Omega\\nabla",
        "\\forall\\epsilon>0,\\exists\\delta>0",
        "\\pm\\mp\\times\\div\\leq\\geq\\neq\\approx",
        "\\frac{d}{dx}\\int_a^b f(t)\\,dt",
        "f(x)=\\sum_{n=0}^{\\infty}\\frac{f^{(n)}(a)}{n!}(x-a)^n",
        "e^{i\\pi}+1=0",
        "\\sqrt{\\frac{a^2+b^2}{c^2}}",
        "\\frac{a}{b}=\\frac{c}{d}",
    ]
    .iter()
    .map(|source| {
        let math = mt_tex::parse(source).unwrap_or_else(|error| panic!("`{source}`: {error}"));
        Case {
            label: source,
            math,
            exception: None,
        }
    })
    .collect()
}

/// Corpus entries whose documented read-back differs from the tree, built directly.
fn exception_cases() -> Vec<Case> {
    vec![
        // A style override has no record; a font override is applied to the characters.
        lost(
            "font override",
            styled(Node::variable('v'), None, Some(Font::Bold), None),
            ch('v', Class::Ord, Font::Bold),
        ),
        // A style override (script size) has no record of its own.
        lost(
            "style override",
            styled(
                Node::variable('x'),
                Some(mt_core::MathStyle::Script),
                None,
                None,
            ),
            Node::variable('x'),
        ),
        // A class override is dropped: the class comes from the glyph.
        lost(
            "class override",
            styled(
                ch('x', Class::Rel, Font::Italic),
                None,
                None,
                Some(Class::Rel),
            ),
            Node::variable('x'),
        ),
        // The class of a character is recomputed from the glyph on reading.
        lost(
            "recomputed class",
            ch('\u{2211}', Class::Bin, Font::Upright),
            ch('\u{2211}', Class::Op, Font::Upright),
        ),
        // Literal text and an unrepresentable construct read back as characters.
        lost(
            "text run",
            Node::Text(Box::new(TextRun {
                text: "a+b".to_owned(),
                font: Font::Upright,
            })),
            Node::row([
                ch('a', Class::Ord, Font::Upright),
                ch('+', Class::Bin, Font::Upright),
                ch('b', Class::Ord, Font::Upright),
            ]),
        ),
        lost(
            "error node",
            Node::Error("??".to_owned()),
            Node::row([
                ch('?', Class::Ord, Font::Upright),
                ch('?', Class::Ord, Font::Upright),
            ]),
        ),
        // Explicit spacing carries no width: every kind reads back as a thin space.
        lost(
            "wide space",
            Node::row([Node::Space(Space::Quad), Node::variable('x')]),
            Node::row([Node::Space(Space::Thin), Node::variable('x')]),
        ),
        // A custom delimiter spelling a drawn fence reads back as that fence.
        lost(
            "custom fence glyphs",
            fence(Delim::Custom('('), Delim::Custom(')'), Node::variable('x')),
            fence(Delim::Paren, Delim::Paren, Node::variable('x')),
        ),
        // Norm and double bar are spellings of the same template, as are arrow and angle.
        lost(
            "norm delimiter",
            fence(Delim::Norm, Delim::Norm, Node::variable('x')),
            fence(Delim::DoubleBar, Delim::DoubleBar, Node::variable('x')),
        ),
        lost(
            "arrow delimiter",
            fence(Delim::Arrow, Delim::Arrow, Node::variable('x')),
            fence(Delim::Angle, Delim::Angle, Node::variable('x')),
        ),
        // A large operator spelled with more than one glyph is written as limits material.
        lost(
            "named limit",
            big(
                Node::Text(Box::new(TextRun {
                    text: "lim".to_owned(),
                    font: Font::Upright,
                })),
                Some(Node::variable('x')),
                None,
                true,
            ),
            limits(
                Node::row([
                    ch('l', Class::Ord, Font::Upright),
                    ch('i', Class::Ord, Font::Upright),
                    ch('m', Class::Ord, Font::Upright),
                ]),
                Some(Node::variable('x')),
                None,
            ),
        ),
    ]
}

/// Corpus entries that round trip exactly, built directly.
fn exact_cases() -> Vec<Case> {
    let mut cases = vec![
        case("bare variable", Node::variable('x')),
        case("empty equation", Node::empty()),
        // Fractions of every kind, nested.
        case(
            "bar fraction",
            frac(Node::variable('a'), Node::variable('b'), FracKind::Bar),
        ),
        case(
            "slash fraction",
            frac(Node::variable('a'), Node::variable('b'), FracKind::Slash),
        ),
        case(
            "linear fraction",
            frac(Node::variable('a'), Node::variable('b'), FracKind::Linear),
        ),
        case(
            "barless fraction",
            frac(Node::variable('a'), Node::variable('b'), FracKind::NoBar),
        ),
        case(
            "nested fractions",
            frac(
                frac(Node::variable('a'), Node::variable('b'), FracKind::Slash),
                frac(Node::variable('c'), Node::variable('d'), FracKind::NoBar),
                FracKind::Bar,
            ),
        ),
        // Radicals with and without an index.
        case(
            "square root",
            Node::Sqrt(Box::new(Sqrt::square(Node::variable('x')))),
        ),
        case(
            "nth root",
            Node::Sqrt(Box::new(Sqrt::nth(
                Node::variable('n'),
                Node::variable('x'),
            ))),
        ),
        case(
            "root of a fraction",
            Node::Sqrt(Box::new(Sqrt::square(frac(
                Node::variable('a'),
                Node::variable('b'),
                FracKind::Bar,
            )))),
        ),
        // Scripts: single-sided, combined, empty slots, on a structured base.
        case(
            "subscript only",
            scripts(Node::variable('x'), Some(Node::variable('i')), None),
        ),
        case(
            "superscript only",
            scripts(Node::variable('x'), None, Some(Node::variable('n'))),
        ),
        case(
            "combined scripts",
            scripts(
                frac(Node::variable('a'), Node::variable('b'), FracKind::Bar),
                Some(Node::variable('i')),
                Some(Node::row([
                    Node::variable('n'),
                    Node::symbol('+', Class::Bin),
                    Node::variable('1'),
                ])),
            ),
        ),
        case(
            "empty superscript",
            scripts(Node::variable('a'), None, Some(Node::empty())),
        ),
        case(
            "empty subscript",
            scripts(Node::variable('a'), Some(Node::empty()), None),
        ),
        case(
            "scripts on empty base",
            scripts(Node::empty(), Some(Node::variable('x')), None),
        ),
        case(
            "stacked scripts",
            scripts(
                scripts(Node::variable('x'), Some(Node::variable('i')), None),
                None,
                Some(Node::variable('2')),
            ),
        ),
        // Limits: under, over, both, and an empty drawn slot.
        case(
            "under limit",
            limits(Node::variable('x'), Some(Node::variable('a')), None),
        ),
        case(
            "over limit",
            limits(Node::variable('x'), None, Some(Node::variable('b'))),
        ),
        case(
            "under and over",
            limits(
                Node::variable('x'),
                Some(Node::variable('a')),
                Some(Node::variable('b')),
            ),
        ),
        case(
            "empty under limit",
            limits(Node::variable('x'), Some(Node::empty()), None),
        ),
        // Fences: every named delimiter, mixed, one-sided and custom pairs.
        case(
            "parentheses",
            fence(Delim::Paren, Delim::Paren, Node::variable('x')),
        ),
        case(
            "brackets",
            fence(Delim::Bracket, Delim::Bracket, Node::variable('x')),
        ),
        case(
            "braces",
            fence(Delim::Brace, Delim::Brace, Node::variable('x')),
        ),
        case("bars", fence(Delim::Abs, Delim::Abs, Node::variable('x'))),
        case(
            "double bars",
            fence(Delim::DoubleBar, Delim::DoubleBar, Node::variable('x')),
        ),
        case(
            "floor",
            fence(Delim::Floor, Delim::Floor, Node::variable('x')),
        ),
        case(
            "ceiling",
            fence(Delim::Ceil, Delim::Ceil, Node::variable('x')),
        ),
        case(
            "angle brackets",
            fence(Delim::Angle, Delim::Angle, Node::variable('x')),
        ),
        case(
            "mixed delimiters",
            fence(Delim::Paren, Delim::Bracket, Node::variable('x')),
        ),
        case(
            "left only",
            fence(Delim::Brace, Delim::None, Node::variable('x')),
        ),
        case(
            "right only",
            fence(Delim::None, Delim::DoubleBar, Node::variable('x')),
        ),
        case("box", fence(Delim::None, Delim::None, Node::variable('x'))),
        case(
            "custom delimiters",
            fence(
                Delim::Custom('\u{2191}'),
                Delim::Custom('\u{2193}'),
                Node::variable('x'),
            ),
        ),
        case(
            "reversed interval",
            fence(Delim::Custom(')'), Delim::Custom('('), Node::variable('x')),
        ),
        // Tables: with delimiters, with rules, stacked, and a long division.
        case(
            "table with delimiters",
            table(
                vec![
                    vec![Node::variable('a'), Node::variable('b')],
                    vec![Node::variable('c'), Node::variable('d')],
                ],
                Some((Delim::Paren, Delim::Paren)),
                MatrixKind::Matrix,
                Side::Center,
                false,
            ),
        ),
        case(
            "table with rules",
            table(
                vec![
                    vec![Node::variable('a'), Node::variable('b')],
                    vec![Node::variable('c'), Node::variable('d')],
                ],
                None,
                MatrixKind::Array,
                Side::Center,
                true,
            ),
        ),
        case(
            "left aligned table",
            table(
                vec![vec![Node::variable('a')], vec![Node::variable('b')]],
                None,
                MatrixKind::Array,
                Side::Left,
                true,
            ),
        ),
        case(
            "right aligned stack",
            table(
                vec![vec![Node::variable('a')], vec![Node::variable('b')]],
                None,
                MatrixKind::Pile,
                Side::Right,
                false,
            ),
        ),
        case(
            "long division",
            table(
                vec![
                    vec![Node::row([Node::variable('a'), Node::variable('b')])],
                    vec![Node::variable('c')],
                ],
                None,
                MatrixKind::LongDivision,
                Side::Center,
                false,
            ),
        ),
        // Large operators: each selector, with and without limits, with and without scripts.
        case(
            "sum with limits",
            Node::row([
                big(
                    ch('\u{2211}', Class::Op, Font::Upright),
                    Some(Node::variable('i')),
                    Some(Node::variable('n')),
                    true,
                ),
                Node::variable('a'),
            ]),
        ),
        case(
            "sum without limits",
            Node::row([
                big(
                    ch('\u{2211}', Class::Op, Font::Upright),
                    Some(Node::variable('i')),
                    None,
                    false,
                ),
                Node::variable('a'),
            ]),
        ),
        case(
            "product with limits",
            Node::row([
                big(
                    ch('\u{220F}', Class::Op, Font::Upright),
                    Some(Node::variable('k')),
                    Some(Node::variable('m')),
                    true,
                ),
                Node::variable('x'),
            ]),
        ),
        case(
            "coproduct with limits",
            Node::row([
                big(
                    ch('\u{2210}', Class::Op, Font::Upright),
                    None,
                    Some(Node::variable('n')),
                    true,
                ),
                Node::variable('x'),
            ]),
        ),
        case(
            "union with limits",
            Node::row([
                big(
                    ch('\u{22C3}', Class::Op, Font::Upright),
                    Some(Node::variable('i')),
                    None,
                    true,
                ),
                Node::variable('A'),
            ]),
        ),
        case(
            "intersection with limits",
            Node::row([
                big(
                    ch('\u{22C2}', Class::Op, Font::Upright),
                    Some(Node::variable('i')),
                    None,
                    true,
                ),
                Node::variable('B'),
            ]),
        ),
        case(
            "integral",
            Node::row([
                big(
                    ch('\u{222B}', Class::Op, Font::Upright),
                    Some(Node::variable('a')),
                    Some(Node::variable('b')),
                    false,
                ),
                Node::variable('x'),
            ]),
        ),
        case(
            "double integral",
            Node::row([
                big(
                    ch('\u{222C}', Class::Op, Font::Upright),
                    Some(Node::variable('D')),
                    None,
                    false,
                ),
                Node::variable('f'),
            ]),
        ),
        case(
            "triple integral",
            Node::row([
                big(ch('\u{222D}', Class::Op, Font::Upright), None, None, false),
                Node::variable('f'),
            ]),
        ),
        case(
            "contour integral",
            Node::row([
                big(
                    ch('\u{222E}', Class::Op, Font::Upright),
                    Some(Node::variable('C')),
                    None,
                    false,
                ),
                Node::variable('F'),
            ]),
        ),
        case(
            "operator without scripts",
            Node::row([
                big(ch('\u{2211}', Class::Op, Font::Upright), None, None, false),
                Node::variable('a'),
            ]),
        ),
        // Faces: the named styles and the ones that need a font definition.
        case(
            "faces",
            Node::row([
                ch('x', Class::Ord, Font::Upright),
                ch('x', Class::Ord, Font::Italic),
                ch('x', Class::Ord, Font::Bold),
                ch('x', Class::Ord, Font::BoldItalic),
                ch('x', Class::Ord, Font::SansSerif),
                ch('x', Class::Ord, Font::SansSerifBold),
                ch('x', Class::Ord, Font::SansSerifItalic),
                ch('x', Class::Ord, Font::SansSerifBoldItalic),
                ch('x', Class::Ord, Font::Monospace),
                ch('x', Class::Ord, Font::Script),
                ch('x', Class::Ord, Font::BoldScript),
                ch('x', Class::Ord, Font::Fraktur),
                ch('x', Class::Ord, Font::BoldFraktur),
                ch('x', Class::Ord, Font::DoubleStruck),
                ch('x', Class::Ord, Font::Symbol),
                ch('x', Class::Ord, Font::Extra),
            ]),
        ),
        // Explicit thin spacing round trips exactly.
        case(
            "thin space",
            Node::row([Node::Space(Space::Thin), Node::variable('x')]),
        ),
        // A deep mix of everything at once.
        case(
            "deep nesting",
            frac(
                Node::Sqrt(Box::new(Sqrt::nth(
                    Node::variable('n'),
                    scripts(Node::variable('x'), None, Some(Node::variable('2'))),
                ))),
                fence(Delim::Abs, Delim::Abs, Node::variable('z')),
                FracKind::Bar,
            ),
        ),
    ];

    // Accents: every kind, over a group and over a single character, over and under.
    let accents = [
        (AccentKind::Hat, false),
        (AccentKind::WideHat, false),
        (AccentKind::Tilde, false),
        (AccentKind::WideTilde, false),
        (AccentKind::Bar, false),
        (AccentKind::Bar, true),
        (AccentKind::Line, false),
        (AccentKind::Line, true),
        (AccentKind::Vector, false),
        (AccentKind::Vector, true),
        (AccentKind::Dot, false),
        (AccentKind::Dot, true),
        (AccentKind::DoubleDot, false),
        (AccentKind::DoubleDot, true),
        (AccentKind::TripleDot, false),
        (AccentKind::TripleDot, true),
        (AccentKind::Circle, false),
        (AccentKind::Breve, false),
        (AccentKind::Check, false),
        (AccentKind::Acute, false),
        (AccentKind::Grave, false),
        (AccentKind::OverBrace, false),
        (AccentKind::UnderBrace, true),
        (AccentKind::OverBracket, false),
        (AccentKind::UnderBracket, true),
        (AccentKind::OverParen, false),
        (AccentKind::UnderParen, true),
        (AccentKind::Strike, false),
        (AccentKind::ArrowRight, false),
        (AccentKind::ArrowRight, true),
        (AccentKind::ArrowLeft, false),
        (AccentKind::ArrowLeft, true),
    ];
    for (kind, under) in accents {
        cases.push(case(
            "accent over a group",
            accent(
                kind,
                Node::row([Node::variable('x'), Node::variable('y')]),
                under,
            ),
        ));
        cases.push(case(
            "accent over a character",
            accent(kind, Node::variable('x'), under),
        ));
    }
    // Two accents on one character, written as one embellishment list.
    cases.push(case(
        "stacked accents",
        accent(
            AccentKind::Hat,
            accent(AccentKind::Tilde, Node::variable('x'), false),
            false,
        ),
    ));

    cases
}

#[test]
fn corpus_round_trips_byte_for_byte() {
    let mut cases = latex_cases();
    cases.extend(exception_cases());
    cases.extend(exact_cases());
    assert!(
        cases.len() >= 50,
        "the corpus covers the constructs the format spells"
    );
    for case in &cases {
        check(case);
    }
}

#[test]
fn corpus_size_is_reported() {
    let mut cases = latex_cases();
    cases.extend(exception_cases());
    cases.extend(exact_cases());
    let exact = cases.iter().filter(|case| case.exception.is_none()).count();
    assert!(
        exact >= 50,
        "{exact} of {} corpus entries round trip exactly; the rest are documented exceptions",
        cases.len(),
    );
}
