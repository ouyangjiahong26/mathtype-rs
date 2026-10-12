//! Round-trip fixtures: `write` produces a document `parse` reads back to the same tree.

use mt_core::{
    Accent, AccentKind, BigOp, Char, Class, Delim, Fence, Font, Frac, FracKind, Limits, Math,
    MathStyle, Matrix, MatrixKind, Node, Row, Scripts, Side, Space, Sqrt, Styled, TextRun,
};
use mt_mathml::{parse, write};

fn variable(ch: char) -> Node {
    Node::variable(ch)
}

fn symbol(ch: char, class: Class) -> Node {
    Node::symbol(ch, class)
}

fn face(ch: char, class: Class, font: Font) -> Node {
    Node::Char(Char { ch, class, font })
}

fn digit(ch: char) -> Node {
    face(ch, Class::Ord, Font::Upright)
}

fn row(items: impl IntoIterator<Item = Node>) -> Node {
    Node::row(items)
}

fn fenced(open: Delim, close: Delim, body: Node) -> Node {
    Node::Fence(Box::new(Fence { open, close, body }))
}

/// Writes `equation`, reads it back and asserts the trees agree after normalisation.
fn round_trip(equation: &Math) {
    let document = write(equation);
    let mut expected = equation.clone();
    expected.body.normalize();
    let mut actual =
        parse(&document).unwrap_or_else(|error| panic!("cannot read back {document}: {error}"));
    actual.body.normalize();
    assert_eq!(actual, expected, "document: {document}");
}

#[test]
fn characters_round_trip_through_their_token_elements() {
    for body in [
        variable('x'),
        digit('7'),
        symbol('+', Class::Bin),
        symbol('=', Class::Rel),
        symbol('∞', Class::Ord),
        symbol('(', Class::Open),
        symbol(')', Class::Close),
        symbol('∑', Class::Op),
        symbol('<', Class::Rel),
        symbol('&', Class::Ord),
    ] {
        round_trip(&Math::new(body));
    }
}

#[test]
fn a_class_that_disagrees_with_the_character_is_recorded() {
    for body in [
        face('(', Class::Ord, Font::Upright),
        face('|', Class::Ord, Font::Upright),
        face('a', Class::Bin, Font::Upright),
        face('+', Class::Ord, Font::Italic),
        face('1', Class::Inner, Font::Upright),
    ] {
        round_trip(&Math::new(body));
    }
}

#[test]
fn every_face_round_trips() {
    for font in [
        Font::Italic,
        Font::Upright,
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
    ] {
        round_trip(&Math::new(face('R', Class::Ord, font)));
        round_trip(&Math::new(Node::Text(Box::new(TextRun {
            text: "total".to_owned(),
            font,
        }))));
    }
}

#[test]
fn literal_text_round_trips_including_markup_characters() {
    for text in [
        "sin",
        "where",
        "",
        " ",
        "a < b & c > \"d\" 'e'",
        "&amp;",
        "<mrow/>",
    ] {
        round_trip(&Math::new(Node::text(text)));
    }
}

#[test]
fn every_fraction_kind_round_trips() {
    for kind in [
        FracKind::Bar,
        FracKind::Slash,
        FracKind::NoBar,
        FracKind::Linear,
    ] {
        round_trip(&Math::new(Node::Frac(Box::new(Frac {
            num: digit('1'),
            den: variable('x'),
            kind,
        }))));
    }
}

#[test]
fn radicals_round_trip_with_and_without_an_index() {
    round_trip(&Math::new(Node::Sqrt(Box::new(Sqrt::square(row([
        variable('x'),
        symbol('+', Class::Bin),
        digit('1'),
    ]))))));
    round_trip(&Math::new(Node::Sqrt(Box::new(Sqrt::nth(
        digit('3'),
        variable('x'),
    )))));
    round_trip(&Math::new(Node::Sqrt(Box::new(Sqrt {
        index: None,
        radicand: Row::new().into(),
    }))));
}

#[test]
fn scripts_round_trip_in_every_combination() {
    let base = || variable('x');
    for (sub, sup) in [
        (None, None),
        (Some(digit('1')), None),
        (None, Some(digit('2'))),
        (Some(digit('1')), Some(digit('2'))),
    ] {
        round_trip(&Math::new(Node::Scripts(Box::new(Scripts {
            base: base(),
            sub,
            sup,
        }))));
    }
}

#[test]
fn large_operators_round_trip_with_and_without_limits() {
    for limits in [true, false] {
        for (sub, sup) in [
            (None, None),
            (Some(digit('0')), None),
            (None, Some(variable('n'))),
            (Some(digit('0')), Some(variable('n'))),
        ] {
            round_trip(&Math::new(Node::Big(Box::new(BigOp {
                base: symbol('∑', Class::Op),
                sub,
                sup,
                limits,
            }))));
        }
    }
}

#[test]
fn limits_round_trip_with_every_alignment() {
    for align in [Side::Center, Side::Left, Side::Right] {
        for (under, over) in [
            (None, None),
            (Some(variable('y')), None),
            (None, Some(variable('y'))),
            (Some(variable('y')), Some(digit('2'))),
        ] {
            round_trip(&Math::new(Node::Limits(Box::new(Limits {
                base: variable('x'),
                under,
                over,
                align,
            }))));
        }
    }
}

#[test]
fn every_delimiter_pair_round_trips() {
    for delim in [
        Delim::Paren,
        Delim::Bracket,
        Delim::Brace,
        Delim::Abs,
        Delim::Norm,
        Delim::Angle,
        Delim::Floor,
        Delim::Ceil,
        Delim::None,
        Delim::Arrow,
        Delim::Custom('⋄'),
        Delim::Custom('&'),
    ] {
        round_trip(&Math::new(fenced(
            delim,
            delim,
            row([variable('x'), symbol('+', Class::Bin), digit('1')]),
        )));
    }
}

#[test]
fn a_fence_body_that_is_not_a_row_round_trips() {
    round_trip(&Math::new(fenced(
        Delim::Paren,
        Delim::Paren,
        variable('x'),
    )));
    round_trip(&Math::new(fenced(
        Delim::Paren,
        Delim::Paren,
        Row::new().into(),
    )));
}

#[test]
fn every_accent_shape_round_trips_on_both_sides() {
    let shapes = [
        AccentKind::Hat,
        AccentKind::WideHat,
        AccentKind::Tilde,
        AccentKind::WideTilde,
        AccentKind::Bar,
        AccentKind::Vector,
        AccentKind::Dot,
        AccentKind::DoubleDot,
        AccentKind::TripleDot,
        AccentKind::Breve,
        AccentKind::Check,
        AccentKind::Acute,
        AccentKind::Grave,
        AccentKind::Circle,
        AccentKind::OverBrace,
        AccentKind::UnderBrace,
        AccentKind::OverBracket,
        AccentKind::UnderBracket,
        AccentKind::OverParen,
        AccentKind::UnderParen,
        AccentKind::ArrowRight,
        AccentKind::ArrowLeft,
        AccentKind::Line,
        AccentKind::Strike,
    ];
    for kind in shapes {
        for under in [false, true] {
            round_trip(&Math::new(Node::Accent(Box::new(Accent {
                kind,
                base: variable('x'),
                under,
            }))));
        }
    }
}

#[test]
fn matrices_round_trip_with_delimiters_alignment_and_rules() {
    let mut matrix = Matrix::empty(2, 2);
    matrix.cell_at_mut(0).map(|cell| *cell = digit('1'));
    matrix.cell_at_mut(3).map(|cell| *cell = variable('x'));
    round_trip(&Math::new(Node::Matrix(Box::new(matrix.clone()))));

    matrix.delims = Some((Delim::Bracket, Delim::Bracket));
    round_trip(&Math::new(Node::Matrix(Box::new(matrix.clone()))));

    matrix.columns = vec![Side::Left, Side::Center];
    matrix.rules = true;
    round_trip(&Math::new(Node::Matrix(Box::new(matrix.clone()))));

    matrix.columns = Vec::new();
    matrix.align = Side::Right;
    round_trip(&Math::new(Node::Matrix(Box::new(matrix))));
}

#[test]
fn every_table_kind_round_trips() {
    for kind in [
        MatrixKind::Matrix,
        MatrixKind::Cases,
        MatrixKind::Aligned,
        MatrixKind::Array,
        MatrixKind::Pile,
        MatrixKind::LongDivision,
    ] {
        let mut matrix = Matrix::empty(1, 1);
        matrix.cell_at_mut(0).map(|cell| *cell = digit('1'));
        matrix.kind = kind;
        round_trip(&Math::new(Node::Matrix(Box::new(matrix))));
    }
}

#[test]
fn empty_tables_and_empty_rows_round_trip() {
    round_trip(&Math::new(Node::Matrix(Box::new(Matrix::empty(0, 0)))));
    let mut matrix = Matrix::empty(1, 0);
    matrix.rows.push(Row::new());
    round_trip(&Math::new(Node::Matrix(Box::new(matrix))));
}

#[test]
fn styles_round_trip_inside_a_row() {
    for style in [
        MathStyle::Full,
        MathStyle::Text,
        MathStyle::Script,
        MathStyle::ScriptScript,
    ] {
        round_trip(&Math::new(row([
            variable('a'),
            Node::Styled(Box::new(Styled {
                body: variable('x'),
                style: Some(style),
                font: None,
                class: None,
            })),
        ])));
    }
    for font in [
        None,
        Some(Font::Bold),
        Some(Font::Monospace),
        Some(Font::Symbol),
        Some(Font::Extra),
    ] {
        round_trip(&Math::new(row([
            variable('a'),
            Node::Styled(Box::new(Styled {
                body: variable('x'),
                style: None,
                font,
                class: None,
            })),
        ])));
    }
    for class in [Class::Ord, Class::Op, Class::Bin, Class::Rel, Class::Inner] {
        round_trip(&Math::new(row([
            variable('a'),
            Node::Styled(Box::new(Styled {
                body: variable('x'),
                style: None,
                font: None,
                class: Some(class),
            })),
        ])));
    }
}

#[test]
fn a_bare_style_wrapper_round_trips() {
    round_trip(&Math::new(row([
        variable('a'),
        Node::Styled(Box::new(Styled {
            body: variable('x'),
            style: None,
            font: None,
            class: None,
        })),
    ])));
}

#[test]
fn every_space_width_round_trips() {
    for space in [
        Space::Thin,
        Space::Medium,
        Space::Thick,
        Space::Sixteenth,
        Space::Quad,
        Space::DoubleQuad,
        Space::NegativeThin,
        Space::NegativeMedium,
        Space::NegativeThick,
    ] {
        round_trip(&Math::new(row([
            variable('a'),
            Node::Space(space),
            variable('b'),
        ])));
    }
}

#[test]
fn the_equation_style_round_trips() {
    for style in [
        MathStyle::Full,
        MathStyle::Text,
        MathStyle::Script,
        MathStyle::ScriptScript,
    ] {
        round_trip(&Math {
            body: row([variable('a'), symbol('=', Class::Rel), variable('b')]),
            style,
        });
    }
}

#[test]
fn alignment_points_and_kept_markup_round_trip() {
    round_trip(&Math::new(row([variable('a'), Node::Align, variable('b')])));
    round_trip(&Math::new(Node::Error("\\frac{".to_owned())));
    round_trip(&Math::new(Node::Error("x < y & \"z\"".to_owned())));
}

#[test]
fn nested_rows_normalise_to_the_same_shape() {
    round_trip(&Math::new(row([
        row([variable('a'), variable('b')]),
        variable('c'),
    ])));
    round_trip(&Math::new(row([Row::new().into()])));
}

#[test]
fn a_nested_expression_round_trips_as_a_whole() {
    let equation = Math::new(row([
        variable('x'),
        symbol('=', Class::Rel),
        Node::Frac(Box::new(Frac {
            num: Node::Sqrt(Box::new(Sqrt::nth(
                digit('3'),
                row([variable('a'), symbol('+', Class::Bin), digit('1')]),
            ))),
            den: Node::Big(Box::new(BigOp {
                base: symbol('∑', Class::Op),
                sub: Some(digit('0')),
                sup: Some(variable('n')),
                limits: true,
            })),
            kind: FracKind::Bar,
        })),
        Node::Scripts(Box::new(Scripts {
            base: fenced(
                Delim::Paren,
                Delim::Paren,
                row([variable('a'), symbol('+', Class::Bin), variable('b')]),
            ),
            sub: Some(digit('2')),
            sup: None,
        })),
        Node::Text(Box::new(TextRun {
            text: "where a < b".to_owned(),
            font: Font::Upright,
        })),
    ]));
    round_trip(&equation);
}

#[test]
fn a_text_base_with_limits_stays_a_large_operator() {
    // `\lim` and friends: the writer spells `Node::Big` over a text base as under/over
    // elements, so the reader must not demote it to `Node::Limits` (or `Node::Scripts`).
    let base = || {
        Node::Text(Box::new(TextRun {
            text: "lim".to_owned(),
            font: Font::Upright,
        }))
    };
    for big in [
        Node::Big(Box::new(BigOp {
            base: base(),
            sub: None,
            sup: None,
            limits: true,
        })),
        Node::Big(Box::new(BigOp {
            base: base(),
            sub: Some(digit('0')),
            sup: None,
            limits: true,
        })),
        Node::Big(Box::new(BigOp {
            base: base(),
            sub: None,
            sup: Some(digit('2')),
            limits: true,
        })),
        Node::Big(Box::new(BigOp {
            base: base(),
            sub: Some(digit('0')),
            sup: Some(digit('2')),
            limits: true,
        })),
        // `\lim\nolimits_0`: no limits, spelled with sub/sup elements.
        Node::Big(Box::new(BigOp {
            base: base(),
            sub: Some(digit('0')),
            sup: None,
            limits: false,
        })),
    ] {
        round_trip(&Math::new(big));
    }
    // A genuine limit over a character base keeps its variant.
    round_trip(&Math::new(Node::Limits(Box::new(Limits {
        base: variable('x'),
        under: None,
        over: Some(digit('2')),
        align: Side::Center,
    }))));
}
