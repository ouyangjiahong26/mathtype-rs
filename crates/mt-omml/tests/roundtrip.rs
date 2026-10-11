//! Round-trip fixtures: `write` produces a document `parse` reads back to the same tree.
//!
//! Office Math carries no size preset, so the equation style is compared separately: a written
//! equation always reads back at full size.

use mt_core::{
    Accent, AccentKind, BigOp, Char, Class, Delim, Fence, Font, Frac, FracKind, Limits, Math, MathStyle, Matrix,
    MatrixKind, Node, Row, Scripts, Side, Space, Sqrt, Styled, TextRun,
};
use mt_omml::{parse, write, write_paragraph};

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

fn text(text: &str, font: Font) -> Node {
    Node::Text(Box::new(TextRun { text: text.to_owned(), font }))
}

/// Writes `equation`, reads it back and asserts the bodies agree after normalisation.
fn round_trip(equation: &Math) {
    round_trip_with(&write(equation), equation);
}

/// The same check against a document the caller already built.
fn round_trip_with(document: &str, equation: &Math) {
    let mut expected = equation.clone();
    expected.body.normalize();
    let mut actual = parse(document).unwrap_or_else(|error| panic!("cannot read back {document}: {error}"));
    assert_eq!(actual.style, MathStyle::Full, "document: {document}");
    actual.body.normalize();
    assert_eq!(actual.body, expected.body, "document: {document}");
}

#[test]
fn characters_round_trip_through_their_runs() {
    for body in [
        variable('x'),
        digit('7'),
        symbol('+', Class::Bin),
        symbol('=', Class::Rel),
        symbol('(', Class::Open),
        symbol(')', Class::Close),
        symbol('∑', Class::Op),
        symbol('∞', Class::Ord),
        symbol('<', Class::Rel),
        symbol('&', Class::Ord),
    ] {
        round_trip(&Math::new(body));
    }
}

#[test]
fn every_face_that_office_math_can_name_round_trips() {
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
    ] {
        round_trip(&Math::new(face('R', Class::Ord, font)));
        round_trip(&Math::new(text("total", font)));
    }
}

#[test]
fn literal_text_round_trips_including_markup_characters() {
    for value in ["sin", "where", "", " ", "a < b & c > \"d\" 'e'", "&amp;", "\u{2003}"] {
        round_trip(&Math::new(text(value, Font::Upright)));
    }
}

#[test]
fn every_fraction_kind_round_trips() {
    for kind in [FracKind::Bar, FracKind::Slash, FracKind::NoBar, FracKind::Linear] {
        round_trip(&Math::new(Node::Frac(Box::new(Frac { num: digit('1'), den: variable('x'), kind }))));
    }
    // A slot holding several items keeps them.
    round_trip(&Math::new(Node::Frac(Box::new(Frac {
        num: row([digit('1'), symbol('+', Class::Bin), variable('x')]),
        den: digit('2'),
        kind: FracKind::Bar,
    }))));
}

#[test]
fn radicals_round_trip_with_and_without_an_index() {
    round_trip(&Math::new(Node::Sqrt(Box::new(Sqrt::square(row([variable('x'), symbol('+', Class::Bin), digit('1')]))))));
    round_trip(&Math::new(Node::Sqrt(Box::new(Sqrt::nth(digit('3'), variable('x'))))));
    round_trip(&Math::new(Node::Sqrt(Box::new(Sqrt { index: None, radicand: Row::new().into() }))));
}

#[test]
fn scripts_round_trip_in_every_combination() {
    for (sub, sup) in [
        (None, None),
        (Some(digit('1')), None),
        (None, Some(digit('2'))),
        (Some(digit('1')), Some(digit('2'))),
    ] {
        round_trip(&Math::new(Node::Scripts(Box::new(Scripts { base: variable('x'), sub, sup }))));
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
fn limits_round_trip_in_every_combination() {
    for (under, over) in [
        (None, None),
        (Some(variable('y')), None),
        (None, Some(variable('y'))),
        (Some(variable('y')), Some(digit('2'))),
        (Some(row([variable('y'), variable('z')])), Some(digit('2'))),
    ] {
        round_trip(&Math::new(Node::Limits(Box::new(Limits {
            base: variable('x'),
            under,
            over,
            align: Side::Center,
        }))));
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
        round_trip(&Math::new(fenced(delim, delim, row([variable('x'), symbol('+', Class::Bin), digit('1')]))));
    }
}

#[test]
fn a_fence_body_that_is_not_a_row_round_trips() {
    round_trip(&Math::new(fenced(Delim::Paren, Delim::Paren, variable('x'))));
    round_trip(&Math::new(fenced(Delim::Paren, Delim::Paren, Row::new().into())));
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
        // A brace-like accent is drawn on the side its kind names, so the side follows the kind.
        let braced = matches!(
            kind,
            AccentKind::OverBrace
                | AccentKind::UnderBrace
                | AccentKind::OverBracket
                | AccentKind::UnderBracket
                | AccentKind::OverParen
                | AccentKind::UnderParen
        );
        let named_under = matches!(kind, AccentKind::UnderBrace | AccentKind::UnderBracket | AccentKind::UnderParen);
        for under in [false, true] {
            if braced && under != named_under {
                continue;
            }
            round_trip(&Math::new(Node::Accent(Box::new(Accent { kind, base: variable('x'), under }))));
        }
    }
}

#[test]
fn an_under_accent_is_written_as_a_grouped_character_not_a_position_on_acc() {
    let accented = Node::Accent(Box::new(Accent { kind: AccentKind::Hat, base: variable('x'), under: true }));
    let document = write(&Math::new(accented.clone()));
    // `m:acc` has no position attribute in the schema, so the mark below the base is grouped
    // under it instead; the position lives where the schema allows it.
    assert!(
        document.contains("<m:groupChr><m:groupChrPr><m:chr m:val=\"^\"/><m:pos m:val=\"bot\"/></m:groupChrPr>"),
        "{document}"
    );
    assert!(!document.contains("<m:acc"), "{document}");
    assert_eq!(parse(&document).unwrap().body, accented);

    // An accent above the base keeps the plain `m:acc` spelling, without any position.
    let over = Node::Accent(Box::new(Accent { kind: AccentKind::Hat, base: variable('x'), under: false }));
    let document = write(&Math::new(over.clone()));
    assert!(document.contains("<m:acc><m:accPr><m:chr m:val=\"^\"/></m:accPr>"), "{document}");
    assert!(!document.contains("m:pos"), "{document}");
    assert_eq!(parse(&document).unwrap().body, over);
}

#[test]
fn a_brace_accent_drawn_on_the_other_side_reads_back_as_that_side_variant() {
    let accented = Node::Accent(Box::new(Accent { kind: AccentKind::OverBrace, base: variable('x'), under: true }));
    let document = write(&Math::new(accented));
    assert_eq!(
        parse(&document).unwrap().body,
        Node::Accent(Box::new(Accent { kind: AccentKind::UnderBrace, base: variable('x'), under: true }))
    );
}

#[test]
fn matrices_round_trip_with_delimiters_and_alignment() {
    let mut matrix = Matrix::empty(2, 2);
    matrix.cell_at_mut(0).map(|cell| *cell = digit('1'));
    matrix.cell_at_mut(3).map(|cell| *cell = variable('x'));
    round_trip(&Math::new(Node::Matrix(Box::new(matrix.clone()))));

    matrix.delims = Some((Delim::Bracket, Delim::Bracket));
    round_trip(&Math::new(Node::Matrix(Box::new(matrix.clone()))));

    matrix.columns = vec![Side::Left, Side::Center];
    round_trip(&Math::new(Node::Matrix(Box::new(matrix.clone()))));

    matrix.columns = Vec::new();
    matrix.align = Side::Right;
    round_trip(&Math::new(Node::Matrix(Box::new(matrix.clone()))));

    matrix.align = Side::Center;
    round_trip(&Math::new(Node::Matrix(Box::new(matrix))));
    round_trip(&Math::new(Node::Matrix(Box::new(Matrix::empty(0, 0)))));
}

#[test]
fn an_aligned_single_column_table_round_trips_as_an_equation_array() {
    let mut matrix = Matrix::empty(2, 1);
    matrix.cell_at_mut(0).map(|cell| *cell = variable('a'));
    matrix.cell_at_mut(1).map(|cell| *cell = row([variable('b'), symbol('=', Class::Rel), digit('1')]));
    matrix.kind = MatrixKind::Aligned;
    round_trip(&Math::new(Node::Matrix(Box::new(matrix))));
}

#[test]
fn wrappers_round_trip() {
    round_trip(&Math::new(Node::Styled(Box::new(Styled { body: variable('x'), style: None, font: None, class: None }))));
    round_trip(&Math::new(Node::Styled(Box::new(Styled {
        body: row([variable('x'), symbol('+', Class::Bin), digit('1')]),
        style: None,
        font: None,
        class: Some(Class::Inner),
    }))));
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
        round_trip(&Math::new(row([variable('a'), Node::Space(space), variable('b')])));
    }
}

#[test]
fn alignment_points_round_trip() {
    round_trip(&Math::new(row([variable('a'), Node::Align, variable('b')])));
}

#[test]
fn every_equation_body_shape_round_trips() {
    round_trip(&Math::new(variable('x')));
    round_trip(&Math::new(row([variable('a'), variable('b')])));
    round_trip(&Math::new(Row::new()));
    round_trip(&Math::new(row([row([variable('a'), variable('b')]), variable('c')])));
}

#[test]
fn a_paragraph_round_trips_for_every_justification() {
    let equation = Math::new(row([variable('x'), symbol('=', Class::Rel), digit('1')]));
    for align in [None, Some("center"), Some("left"), Some("right"), Some("centerGroup"), Some("elsewhere")] {
        round_trip_with(&write_paragraph(&equation, align), &equation);
    }
}

#[test]
fn a_nested_expression_round_trips_as_a_whole() {
    let equation = Math::new(row([
        variable('x'),
        symbol('=', Class::Rel),
        Node::Frac(Box::new(Frac {
            num: Node::Sqrt(Box::new(Sqrt::nth(digit('3'), row([variable('a'), symbol('+', Class::Bin), digit('1')])))),
            den: Node::Big(Box::new(BigOp {
                base: symbol('∑', Class::Op),
                sub: Some(digit('0')),
                sup: Some(variable('n')),
                limits: true,
            })),
            kind: FracKind::Bar,
        })),
        Node::Scripts(Box::new(Scripts {
            base: fenced(Delim::Paren, Delim::Paren, row([variable('a'), symbol('+', Class::Bin), variable('b')])),
            sub: Some(digit('2')),
            sup: None,
        })),
        text("where a < b", Font::Upright),
    ]));
    round_trip(&equation);
}

// The canonical forms Office Math cannot spell, kept here so the behaviour is pinned down.

#[test]
fn a_row_holding_one_item_reads_back_as_that_item() {
    let document = write(&Math::new(row([variable('x')])));
    assert_eq!(parse(&document).unwrap().body, variable('x'));
}

#[test]
fn a_character_class_that_disagrees_with_the_character_is_canonicalised() {
    let document = write(&Math::new(face('+', Class::Ord, Font::Upright)));
    assert_eq!(parse(&document).unwrap().body, symbol('+', Class::Bin));
}

#[test]
fn a_style_and_a_face_on_a_wrapper_are_canonicalised() {
    let styled = Node::Styled(Box::new(Styled {
        body: variable('x'),
        style: Some(MathStyle::Script),
        font: Some(Font::Bold),
        class: None,
    }));
    let document = write(&Math::new(styled));
    assert_eq!(
        parse(&document).unwrap().body,
        Node::Styled(Box::new(Styled { body: variable('x'), style: None, font: None, class: None }))
    );
}

#[test]
fn a_non_full_size_preset_reads_back_as_full() {
    for style in [MathStyle::Text, MathStyle::Script, MathStyle::ScriptScript] {
        let document = write(&Math { body: variable('x'), style });
        assert_eq!(parse(&document).unwrap().style, MathStyle::Full);
    }
}

#[test]
fn a_kept_markup_node_reads_back_as_literal_text() {
    let document = write(&Math::new(Node::Error("\\frac{".to_owned())));
    assert_eq!(parse(&document).unwrap().body, text("\\frac{", Font::Upright));
}

#[test]
fn a_table_kind_office_math_cannot_name_reads_back_as_a_plain_table() {
    for kind in [MatrixKind::Cases, MatrixKind::Array, MatrixKind::Pile, MatrixKind::LongDivision] {
        let mut matrix = Matrix::empty(1, 1);
        matrix.cell_at_mut(0).map(|cell| *cell = digit('1'));
        matrix.kind = kind;
        let mut expected = matrix.clone();
        expected.kind = MatrixKind::Matrix;
        let document = write(&Math::new(Node::Matrix(Box::new(matrix))));
        assert_eq!(parse(&document).unwrap().body, Node::Matrix(Box::new(expected)));
    }
}

#[test]
fn cell_rules_have_no_spelling_and_read_back_off() {
    let mut matrix = Matrix::empty(1, 1);
    matrix.cell_at_mut(0).map(|cell| *cell = digit('1'));
    matrix.rules = true;
    let mut expected = matrix.clone();
    expected.rules = false;
    let document = write(&Math::new(Node::Matrix(Box::new(matrix))));
    assert_eq!(parse(&document).unwrap().body, Node::Matrix(Box::new(expected)));
}

#[test]
fn a_row_with_no_cells_reads_back_as_one_empty_cell() {
    let mut expected = Matrix::empty(2, 0);
    expected.rows = vec![vec![Row::new().into()], vec![Row::new().into()]];
    let document = write(&Math::new(Node::Matrix(Box::new(Matrix::empty(2, 0)))));
    assert_eq!(parse(&document).unwrap().body, Node::Matrix(Box::new(expected)));
}

#[test]
fn writing_is_idempotent() {
    let equation = Math::new(row([
        variable('x'),
        Node::Frac(Box::new(Frac { num: digit('1'), den: variable('y'), kind: FracKind::Bar })),
        Node::Sqrt(Box::new(Sqrt::square(variable('z')))),
    ]));
    let once = write(&equation);
    assert_eq!(once, write(&parse(&once).unwrap()));
}
