//! Round trips of the equation tree through the equation byte stream.

use mt_core::{
    Accent, AccentKind, BigOp, Char, Class, Delim, Fence, Font, Frac, FracKind, Limits, Math,
    Matrix, MatrixKind, Node, Scripts, Side, Sqrt, Space,
};
use mt_mtef::{parse, parse_bare, write, write_bare};

/// Equations covering every construct the format spells: fractions, radicals, scripts, fences,
/// tables, accents, large operators with limits, and the spacing the format can carry.
const CORPUS: &[&str] = &[
    "a",
    "x+1",
    "2a-b",
    "x=1",
    "(a+b)",
    "\\frac{a}{b}",
    "\\frac{a+b}{c-d}",
    "\\frac{1}{2}",
    "\\frac{x+1}{y-1}",
    "\\sqrt{x}",
    "\\sqrt{x+1}",
    "\\sqrt[3]{x}",
    "\\sqrt[n]{a+b}",
    "x^2",
    "a_i",
    "x_i^2",
    "x^{2n}",
    "e^{-x}",
    "x^{y^{z}}",
    "\\left(a+b\\right)",
    "\\left[\\frac{a}{b}\\right]",
    "\\left\\{x\\right\\}",
    "\\left|x\\right|",
    "\\left\\lfloor x\\right\\rfloor",
    "\\left\\lceil x\\right\\rceil",
    "\\langle x\\rangle",
    "\\binom{n}{k}",
    "\\hat{x}",
    "\\widehat{abc}",
    "\\bar{x}",
    "\\overline{xy}",
    "\\underline{xy}",
    "\\vec{v}",
    "\\dot{x}",
    "\\ddot{x}",
    "\\tilde{x}",
    "\\begin{matrix}a&b\\\\c&d\\end{matrix}",
    "\\begin{pmatrix}a&b\\\\c&d\\end{pmatrix}",
    "\\begin{bmatrix}1&0\\\\0&1\\end{bmatrix}",
    "\\begin{vmatrix}a&b\\\\c&d\\end{vmatrix}",
    "\\begin{pmatrix}1&2&3\\\\4&5&6\\end{pmatrix}",
    "\\begin{pmatrix}\\frac{1}{2}&0\\\\0&\\frac{1}{2}\\end{pmatrix}",
    "\\sum_{i=1}^{n}a_i",
    "\\prod_{k=0}^{n}x_k",
    "\\int_0^1 x",
    "\\bigcup_{i}A_i",
    "\\bigcap_{i}B_i",
    "\\sum_{i=1}^{n}i",
    "\\frac{1}{2}\\sum_{i=1}^{n}\\frac{a_i}{b_i}",
    "\\sqrt{\\frac{a}{b}}",
    "\\left(\\sum_{i=1}^{n}a_i\\right)^2",
    "\\frac{a}{b}=\\frac{c}{d}",
    "\\int_0^1\\frac{x}{x+1}",
    "\\alpha+\\beta=\\gamma",
];

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

fn table(rows: Vec<Vec<Node>>, delims: Option<(Delim, Delim)>, kind: MatrixKind, align: Side, rules: bool) -> Node {
    Node::Matrix(Box::new(Matrix { rows, delims, kind, align, columns: Vec::new(), rules }))
}

/// Assert that a tree survives writing and reading back.
///
/// The format always reads a top-level row, so a lone node is wrapped in one, which is the same
/// equation.
fn check(node: Node, label: &str) {
    let mut expected = match node {
        Node::Row(_) => node,
        other => Node::row([other]),
    };
    expected.normalize();
    let math = Math::new(expected.clone());

    let bare = write_bare(&math);
    let read = parse_bare(&bare).unwrap_or_else(|error| panic!("`{label}`: {error}"));
    let mut body = read.body.clone();
    body.normalize();
    assert_eq!(body, expected, "bare round trip of `{label}`");
    assert_eq!(read.style, math.style, "bare round trip style of `{label}`");

    let stream = write(&math);
    let read = parse(&stream).unwrap_or_else(|error| panic!("`{label}`: {error}"));
    let mut body = read.body;
    body.normalize();
    assert_eq!(body, expected, "stream round trip of `{label}`");
}

#[test]
fn latex_corpus_round_trips() {
    for source in CORPUS {
        let math = mt_tex::parse(source).unwrap_or_else(|error| panic!("`{source}`: {error}"));
        check(math.body, source);
    }
    assert!(CORPUS.len() >= 25, "the corpus covers the constructs the format spells");
}

#[test]
fn trees_built_from_the_model_round_trip() {
    // Faces that need a font definition of their own.
    let faces = [
        Font::Upright,
        Font::Italic,
        Font::Bold,
        Font::BoldItalic,
        Font::SansSerif,
        Font::SansSerifBold,
        Font::SansSerifItalic,
        Font::SansSerifBoldItalic,
        Font::Monospace,
        Font::Script,
        Font::BoldScript,
        Font::Fraktur,
        Font::BoldFraktur,
        Font::DoubleStruck,
        Font::Symbol,
        Font::Extra,
    ];
    check(Node::row(faces.map(|font| ch('x', Class::Ord, font))), "faces");
    check(
        Node::row([
            ch('a', Class::Ord, Font::Italic),
            ch('+', Class::Bin, Font::Upright),
            ch('\u{2211}', Class::Op, Font::Upright),
            ch('\u{222B}', Class::Op, Font::Upright),
            ch('\u{2264}', Class::Rel, Font::Symbol),
            ch('+', Class::Bin, Font::Extra),
        ]),
        "classes",
    );

    // Spacing.
    check(Node::row([Node::Space(Space::Thin), Node::variable('x')]), "spacing");

    // Fences: a matched pair, a mixed pair, delimiters drawn on one side only, none at all,
    // custom characters, and a box.
    let body = Node::variable('x');
    check(fence(Delim::Paren, Delim::Paren, body.clone()), "parentheses");
    check(fence(Delim::Paren, Delim::Bracket, body.clone()), "mixed delimiters");
    check(fence(Delim::Brace, Delim::None, body.clone()), "one-sided fence");
    check(fence(Delim::None, Delim::DoubleBar, body.clone()), "closing fence only");
    check(
        fence(Delim::Custom('\u{2191}'), Delim::Custom('\u{2193}'), body.clone()),
        "custom delimiters",
    );
    check(fence(Delim::None, Delim::None, body), "box");

    // Fractions of every kind.
    for kind in [FracKind::Bar, FracKind::Slash, FracKind::Linear, FracKind::NoBar] {
        check(
            frac(
                Node::variable('a'),
                Node::row([Node::variable('b'), Node::variable('c')]),
                kind,
            ),
            "fraction kind",
        );
    }

    // Radicals, scripts and limits.
    check(
        Node::Sqrt(Box::new(Sqrt::nth(Node::variable('n'), Node::variable('x')))),
        "nth root",
    );
    check(
        Node::Sqrt(Box::new(Sqrt::square(Node::row([Node::variable('a'), Node::variable('b')])))),
        "square root",
    );
    check(
        Node::Scripts(Box::new(Scripts {
            base: Node::variable('x'),
            sub: Some(Node::variable('i')),
            sup: Some(Node::variable('j')),
        })),
        "scripts",
    );
    check(
        Node::Limits(Box::new(Limits {
            base: Node::variable('x'),
            under: Some(Node::variable('a')),
            over: Some(Node::row([Node::variable('b'), Node::symbol('=', Class::Rel)])),
            align: Side::Center,
        })),
        "limits",
    );

    // Accents, over a group and over a single character, in every drawable position.
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
        check(accent(kind, Node::row([Node::variable('x'), Node::variable('y')]), under), "accent over a group");
        check(accent(kind, Node::variable('x'), under), "accent over a character");
    }
    // Two accents on one character, which the format writes as one embellishment list.
    check(
        accent(AccentKind::Hat, accent(AccentKind::Tilde, Node::variable('x'), false), false),
        "stacked accents",
    );

    // Large operators, with and without limits.
    for (operator, limits) in [('\u{2211}', true), ('\u{222B}', false), ('\u{220F}', true)] {
        check(
            Node::row([
                Node::Big(Box::new(BigOp {
                    base: ch(operator, Class::Op, Font::Upright),
                    sub: Some(Node::variable('i')),
                    sup: Some(Node::variable('n')),
                    limits,
                })),
                Node::variable('a'),
            ]),
            "large operator",
        );
    }

    // Tables: with delimiters, drawing cell rules, stacked, and a long division.
    let cells = || vec![vec![Node::variable('a'), Node::variable('b')], vec![Node::variable('c'), Node::variable('d')]];
    check(
        table(cells(), Some((Delim::Paren, Delim::Paren)), MatrixKind::Matrix, Side::Center, false),
        "table with delimiters",
    );
    check(table(cells(), None, MatrixKind::Array, Side::Center, true), "table with rules");
    check(
        table(
            vec![vec![Node::variable('a')], vec![Node::variable('b')], vec![Node::variable('c')]],
            None,
            MatrixKind::Pile,
            Side::Center,
            false,
        ),
        "stack",
    );
    check(
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
        "long division",
    );
    check(table(vec![Vec::new(), Vec::new()], None, MatrixKind::Matrix, Side::Center, false), "empty table");
}
