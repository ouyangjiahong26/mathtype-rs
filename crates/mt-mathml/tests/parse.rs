//! Parse fixtures: each asserts the exact tree a MathML document produces.

use mt_core::{
    Accent, AccentKind, BigOp, Char, Class, Delim, Fence, Font, Frac, FracKind, Limits, Math,
    MathStyle, Matrix, MatrixKind, Node, Row, Scripts, Side, Space, Sqrt, Styled, TextRun,
};
use mt_mathml::parse;

fn math(body: impl Into<Node>) -> Math {
    Math::new(body)
}

fn row(items: impl IntoIterator<Item = Node>) -> Node {
    Node::row(items)
}

fn variable(ch: char) -> Node {
    Node::variable(ch)
}

fn symbol(ch: char, class: Class) -> Node {
    Node::symbol(ch, class)
}

fn digit(ch: char) -> Node {
    Node::Char(Char {
        ch,
        class: Class::Ord,
        font: Font::Upright,
    })
}

fn face(ch: char, font: Font) -> Node {
    Node::Char(Char {
        ch,
        class: Class::Ord,
        font,
    })
}

fn text(text: &str) -> Node {
    Node::text(text)
}

#[test]
fn a_single_token_becomes_the_body() {
    assert_eq!(
        parse("<math><mi>x</mi></math>").unwrap(),
        math(variable('x'))
    );
}

#[test]
fn several_children_become_a_row() {
    assert_eq!(
        parse("<math><mi>a</mi><mo>+</mo><mi>b</mi></math>").unwrap(),
        math(row([variable('a'), symbol('+', Class::Bin), variable('b')]))
    );
}

#[test]
fn a_row_is_kept_as_a_row() {
    assert_eq!(
        parse("<math><mrow><mi>a</mi><mo>+</mo><mi>b</mi></mrow></math>").unwrap(),
        math(row([variable('a'), symbol('+', Class::Bin), variable('b')]))
    );
}

#[test]
fn a_multi_letter_identifier_is_literal_text() {
    assert_eq!(
        parse("<math><mi>sin</mi></math>").unwrap(),
        math(text("sin"))
    );
}

#[test]
fn a_multi_digit_number_is_one_character_per_digit() {
    assert_eq!(
        parse("<math><mn>123</mn></math>").unwrap(),
        math(row([digit('1'), digit('2'), digit('3')]))
    );
}

#[test]
fn operators_take_their_class_from_the_character() {
    assert_eq!(
        parse("<math><mrow><mo>(</mo><mo>=</mo><mo>,</mo><mo>+</mo></mrow></math>").unwrap(),
        math(row([
            symbol('(', Class::Open),
            symbol('=', Class::Rel),
            symbol(',', Class::Punct),
            symbol('+', Class::Bin),
        ]))
    );
}

#[test]
fn a_fenced_operator_is_an_opening_delimiter() {
    assert_eq!(
        parse("<math><mo fence=\"true\">|</mo></math>").unwrap(),
        math(symbol('|', Class::Open))
    );
}

#[test]
fn a_separator_operator_is_punctuation() {
    assert_eq!(
        parse("<math><mo separators=\"true\">+</mo></math>").unwrap(),
        math(symbol('+', Class::Punct))
    );
}

#[test]
fn mathvariant_selects_the_face() {
    assert_eq!(
        parse("<math><mi mathvariant=\"double-struck\">R</mi></math>").unwrap(),
        math(face('R', Font::DoubleStruck))
    );
}

#[test]
fn mtext_keeps_its_escaped_characters() {
    assert_eq!(
        parse("<math><mtext>if x &lt; 1 &amp; y &gt; 0</mtext></math>").unwrap(),
        math(text("if x < 1 & y > 0"))
    );
}

#[test]
fn a_fraction_holds_numerator_and_denominator() {
    assert_eq!(
        parse("<math><mfrac><mn>1</mn><mn>2</mn></mfrac></math>").unwrap(),
        math(Node::Frac(Box::new(Frac {
            num: digit('1'),
            den: digit('2'),
            kind: FracKind::Bar
        })))
    );
}

#[test]
fn fraction_attributes_select_the_rule() {
    let cases = [
        ("<mfrac bevelled=\"true\">", FracKind::Slash),
        ("<mfrac linethickness=\"0\">", FracKind::NoBar),
        (
            "<mfrac bevelled=\"true\" linethickness=\"0\">",
            FracKind::Linear,
        ),
    ];
    for (markup, kind) in cases {
        let document = format!("<math>{markup}<mi>a</mi><mi>b</mi></mfrac></math>");
        assert_eq!(
            parse(&document).unwrap(),
            math(Node::Frac(Box::new(Frac {
                num: variable('a'),
                den: variable('b'),
                kind
            }))),
            "{document}"
        );
    }
}

#[test]
fn a_square_root_and_an_indexed_root() {
    assert_eq!(
        parse("<math><msqrt><mi>x</mi></msqrt></math>").unwrap(),
        math(Node::Sqrt(Box::new(Sqrt::square(variable('x')))))
    );
    // MathML spells `mroot` as the base followed by the index, so `∛x` reads as `\sqrt[3]{x}`.
    assert_eq!(
        parse("<math><mroot><mi>x</mi><mn>3</mn></mroot></math>").unwrap(),
        math(Node::Sqrt(Box::new(Sqrt::nth(digit('3'), variable('x')))))
    );
}

#[test]
fn sub_and_superscripts_attach_to_the_base() {
    assert_eq!(
        parse("<math><msubsup><mi>x</mi><mn>1</mn><mn>2</mn></msubsup></math>").unwrap(),
        math(Node::Scripts(Box::new(Scripts {
            base: variable('x'),
            sub: Some(digit('1')),
            sup: Some(digit('2')),
        })))
    );
    assert_eq!(
        parse("<math><msub><mi>x</mi><mi>i</mi></msub></math>").unwrap(),
        math(Node::Scripts(Box::new(Scripts {
            base: variable('x'),
            sub: Some(variable('i')),
            sup: None
        })))
    );
    assert_eq!(
        parse("<math><msup><mi>x</mi><mn>2</mn></msup></math>").unwrap(),
        math(Node::Scripts(Box::new(Scripts {
            base: variable('x'),
            sub: None,
            sup: Some(digit('2'))
        })))
    );
}

#[test]
fn a_large_operator_with_limits_is_a_big_node() {
    let document = "<math><munderover><mo>∑</mo><mn>0</mn><mi>n</mi></munderover></math>";
    assert_eq!(
        parse(document).unwrap(),
        math(Node::Big(Box::new(BigOp {
            base: symbol('∑', Class::Op),
            sub: Some(digit('0')),
            sup: Some(variable('n')),
            limits: true,
        })))
    );
}

#[test]
fn a_large_operator_with_beside_scripts_is_a_big_node_without_limits() {
    assert_eq!(
        parse("<math><msubsup><mo>∑</mo><mn>0</mn><mi>n</mi></msubsup></math>").unwrap(),
        math(Node::Big(Box::new(BigOp {
            base: symbol('∑', Class::Op),
            sub: Some(digit('0')),
            sup: Some(variable('n')),
            limits: false,
        })))
    );
}

#[test]
fn over_and_under_material_that_is_not_an_accent_is_limits() {
    assert_eq!(
        parse("<math><munder align=\"left\"><mi>x</mi><mi>y</mi></munder></math>").unwrap(),
        math(Node::Limits(Box::new(Limits {
            base: variable('x'),
            under: Some(variable('y')),
            over: None,
            align: Side::Left,
        })))
    );
}

#[test]
fn an_accent_attribute_turns_over_material_into_an_accent() {
    assert_eq!(
        parse("<math><mover accent=\"true\"><mi>x</mi><mo>^</mo></mover></math>").unwrap(),
        math(Node::Accent(Box::new(Accent {
            kind: AccentKind::Hat,
            base: variable('x'),
            under: false
        })))
    );
    assert_eq!(
        parse("<math><munder accentunder=\"true\"><mi>x</mi><mo>⏟</mo></munder></math>").unwrap(),
        math(Node::Accent(Box::new(Accent {
            kind: AccentKind::UnderBrace,
            base: variable('x'),
            under: true
        })))
    );
}

#[test]
fn a_fence_takes_its_delimiters_from_the_attributes() {
    assert_eq!(
        parse("<math><mfenced open=\"{\" close=\"}\"><mi>x</mi></mfenced></math>").unwrap(),
        math(Node::Fence(Box::new(Fence {
            open: Delim::Brace,
            close: Delim::Brace,
            body: variable('x')
        })))
    );
    assert_eq!(
        parse("<math><mfenced open=\"\" close=\"\"><mi>x</mi></mfenced></math>").unwrap(),
        math(Node::Fence(Box::new(Fence {
            open: Delim::None,
            close: Delim::None,
            body: variable('x')
        })))
    );
    assert_eq!(
        parse("<math><mfenced><mi>x</mi></mfenced></math>").unwrap(),
        math(Node::Fence(Box::new(Fence {
            open: Delim::Paren,
            close: Delim::Paren,
            body: variable('x')
        })))
    );
}

#[test]
fn a_table_inside_a_fence_carries_those_delimiters() {
    let document = "<math><mfenced open=\"[\" close=\"]\"><mtable><mtr><mtd><mn>1</mn></mtd></mtr></mtable></mfenced></math>";
    let mut matrix = Matrix::empty(1, 1);
    matrix.cell_at_mut(0).map(|cell| *cell = digit('1'));
    matrix.delims = Some((Delim::Bracket, Delim::Bracket));
    assert_eq!(
        parse(document).unwrap(),
        math(Node::Matrix(Box::new(matrix)))
    );
}

#[test]
fn table_alignment_and_rules_come_from_the_attributes() {
    let document = "<math><mtable columnalign=\"left center\" columnlines=\"solid\"><mtr><mtd><mn>1</mn></mtd><mtd><mn>2</mn></mtd></mtr></mtable></math>";
    let mut matrix = Matrix::empty(1, 2);
    matrix.cell_at_mut(0).map(|cell| *cell = digit('1'));
    matrix.cell_at_mut(1).map(|cell| *cell = digit('2'));
    matrix.columns = vec![Side::Left, Side::Center];
    matrix.rules = true;
    assert_eq!(
        parse(document).unwrap(),
        math(Node::Matrix(Box::new(matrix)))
    );
}

#[test]
fn a_column_alignment_repeated_on_every_cell_folds_into_the_default() {
    let document =
        "<math><mtable><mtr><mtd columnalign=\"right\"><mn>1</mn></mtd></mtr></mtable></math>";
    let mut matrix = Matrix::empty(1, 1);
    matrix.cell_at_mut(0).map(|cell| *cell = digit('1'));
    matrix.align = Side::Right;
    assert_eq!(
        parse(document).unwrap(),
        math(Node::Matrix(Box::new(matrix)))
    );
}

#[test]
fn a_table_kind_marker_is_read_back() {
    let document =
        "<math><mtable class=\"mt-cases\"><mtr><mtd><mn>1</mn></mtd></mtr></mtable></math>";
    let mut matrix = Matrix::empty(1, 1);
    matrix.cell_at_mut(0).map(|cell| *cell = digit('1'));
    matrix.kind = MatrixKind::Cases;
    assert_eq!(
        parse(document).unwrap(),
        math(Node::Matrix(Box::new(matrix)))
    );
}

#[test]
fn style_wrappers_carry_the_size_preset_and_the_face() {
    assert_eq!(
        parse("<math><mrow><mstyle scriptlevel=\"1\" mathvariant=\"bold\"><mi>x</mi></mstyle></mrow></math>").unwrap(),
        math(row([Node::Styled(Box::new(Styled {
            body: variable('x'),
            style: Some(MathStyle::Script),
            font: Some(Font::Bold),
            class: None,
        }))]))
    );
}

#[test]
fn a_lone_style_wrapper_at_the_top_becomes_the_equation_style() {
    assert_eq!(
        parse("<math><mstyle scriptlevel=\"2\"><mi>x</mi></mstyle></math>").unwrap(),
        Math {
            body: variable('x'),
            style: MathStyle::ScriptScript
        }
    );
    assert_eq!(
        parse("<math display=\"inline\"><mi>x</mi></math>").unwrap(),
        Math {
            body: variable('x'),
            style: MathStyle::Text
        }
    );
}

#[test]
fn phantom_padded_and_enclosed_bodies_wrap_their_content() {
    let cases = [
        (
            "<mphantom>",
            "</mphantom>",
            Styled {
                body: variable('x'),
                style: None,
                font: None,
                class: None,
            },
        ),
        (
            "<mpadded>",
            "</mpadded>",
            Styled {
                body: variable('x'),
                style: None,
                font: None,
                class: None,
            },
        ),
        (
            "<menclose notation=\"box\">",
            "</menclose>",
            Styled {
                body: variable('x'),
                style: None,
                font: None,
                class: Some(Class::Inner),
            },
        ),
    ];
    for (open, close, wrapper) in cases {
        let document = format!("<math>{open}<mi>x</mi>{close}</math>");
        assert_eq!(
            parse(&document).unwrap(),
            math(Node::Styled(Box::new(wrapper))),
            "{document}"
        );
    }
}

#[test]
fn mspace_width_maps_onto_the_em_grid() {
    let cases = [
        ("thinmathspace", Space::Thin),
        ("mediummathspace", Space::Medium),
        ("thickmathspace", Space::Thick),
        ("verythickmathspace", Space::Sixteenth),
        ("1em", Space::Quad),
        ("2em", Space::DoubleQuad),
        ("negativethinmathspace", Space::NegativeThin),
        ("negativemediummathspace", Space::NegativeMedium),
        ("negativethickmathspace", Space::NegativeThick),
        ("0.1666em", Space::Thin),
        ("18mu", Space::Quad),
    ];
    for (width, space) in cases {
        let document = format!("<math><mi>a</mi><mspace width=\"{width}\"/><mi>b</mi></math>");
        assert_eq!(
            parse(&document).unwrap(),
            math(row([variable('a'), Node::Space(space), variable('b')])),
            "{document}"
        );
    }
}

#[test]
fn an_alignment_point_and_a_kept_error_are_nodes() {
    assert_eq!(
        parse("<math><mrow><mi>a</mi><malignmark/><mi>b</mi></mrow></math>").unwrap(),
        math(row([variable('a'), Node::Align, variable('b')]))
    );
    assert_eq!(
        parse("<math><merror>\\frac{</merror></math>").unwrap(),
        math(Node::Error("\\frac{".to_owned()))
    );
}

#[test]
fn a_prefixed_document_without_a_display_attribute_is_full_size() {
    let document = "<m:math xmlns:m=\"http://www.w3.org/1998/Math/MathML\"><m:mi>y</m:mi></m:math>";
    assert_eq!(parse(document).unwrap(), math(variable('y')));
}

#[test]
fn whitespace_between_elements_is_not_content() {
    let document = "<math>\n  <mrow>\n    <mi>a</mi>\n    <mi>b</mi>\n  </mrow>\n</math>\n";
    assert_eq!(
        parse(document).unwrap(),
        math(row([variable('a'), variable('b')]))
    );
}

#[test]
fn a_full_document_keeps_every_construct() {
    let text_run = Node::Text(Box::new(TextRun {
        text: "where".to_owned(),
        font: Font::Upright,
    }));
    let document = "\
<math display=\"block\">
  <mrow>
    <mi>x</mi>
    <mo>=</mo>
    <mfrac><mrow><mn>1</mn></mrow><mn>2</mn></mfrac>
    <msqrt><mi>y</mi></msqrt>
    <msub><mi>a</mi><mi>i</mi></msub>
    <mtext>where</mtext>
  </mrow>
</math>";
    assert_eq!(
        parse(document).unwrap(),
        math(row([
            variable('x'),
            symbol('=', Class::Rel),
            Node::Frac(Box::new(Frac {
                num: row([digit('1')]),
                den: digit('2'),
                kind: FracKind::Bar
            })),
            Node::Sqrt(Box::new(Sqrt::square(variable('y')))),
            Node::Scripts(Box::new(Scripts {
                base: variable('a'),
                sub: Some(variable('i')),
                sup: None
            })),
            text_run,
        ]))
    );
}

#[test]
fn a_row_holding_nothing_is_an_empty_body() {
    assert_eq!(parse("<math><mrow/></math>").unwrap(), math(Row::new()));
}
