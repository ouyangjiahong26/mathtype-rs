//! Parse fixtures: each asserts the exact tree an OMML document produces.

use mt_core::{
    Accent, AccentKind, BigOp, Char, Class, Delim, Fence, Font, Frac, FracKind, Limits, Math,
    MathStyle, Matrix, MatrixKind, Node, Scripts, Side, Space, Sqrt, Styled, TextRun,
};
use mt_omml::parse;

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

fn face(ch: char, class: Class, font: Font) -> Node {
    Node::Char(Char { ch, class, font })
}

/// A run with `m:t` text, optionally with properties.
fn run(properties: &str, text: &str) -> String {
    if properties.is_empty() {
        format!("<m:r><m:t>{text}</m:t></m:r>")
    } else {
        format!("<m:r><m:rPr>{properties}</m:rPr><m:t>{text}</m:t></m:r>")
    }
}

/// A document wrapping the given markup in `<m:oMath>`.
fn document(markup: &str) -> String {
    format!(
        "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">{markup}</m:oMath>"
    )
}

#[test]
fn a_single_run_is_the_body() {
    assert_eq!(
        parse(&document(&run("", "x"))).unwrap(),
        math(variable('x'))
    );
}

#[test]
fn a_bare_digit_run_is_upright() {
    assert_eq!(
        parse(&document(&run("", "1"))).unwrap(),
        math(face('1', Class::Ord, Font::Upright))
    );
}

#[test]
fn several_runs_become_a_row() {
    let markup = format!("{}{}{}", run("", "a"), run("", "+"), run("", "b"));
    assert_eq!(
        parse(&document(&markup)).unwrap(),
        math(row([variable('a'), symbol('+', Class::Bin), variable('b')]))
    );
}

#[test]
fn a_normal_text_run_is_literal_text() {
    assert_eq!(
        parse(&document(&run("<m:nor m:val=\"1\"/>", "sin"))).unwrap(),
        math(Node::Text(Box::new(TextRun {
            text: "sin".to_owned(),
            font: Font::Upright
        })))
    );
    assert_eq!(
        parse(&document(&run(
            "<m:nor m:val=\"1\"/><m:sty m:val=\"i\"/>",
            "x"
        )))
        .unwrap(),
        math(Node::Text(Box::new(TextRun {
            text: "x".to_owned(),
            font: Font::Italic
        })))
    );
}

#[test]
fn a_run_whose_text_is_not_a_letter_is_literal_text() {
    assert_eq!(
        parse(&document(&run("", "abc"))).unwrap(),
        math(Node::Text(Box::new(TextRun {
            text: "abc".to_owned(),
            font: Font::Upright
        })))
    );
}

#[test]
fn run_properties_select_the_face() {
    let cases = [
        ("<m:sty m:val=\"p\"/>", Font::Upright),
        ("<m:sty m:val=\"b\"/>", Font::Bold),
        ("<m:sty m:val=\"bi\"/>", Font::BoldItalic),
        ("<m:sty m:val=\"i\"/>", Font::Italic),
        (
            "<m:scr m:val=\"double-struck\"/><m:sty m:val=\"p\"/>",
            Font::DoubleStruck,
        ),
        (
            "<m:scr m:val=\"fraktur\"/><m:sty m:val=\"b\"/>",
            Font::BoldFraktur,
        ),
        (
            "<m:scr m:val=\"script\"/><m:sty m:val=\"p\"/>",
            Font::Script,
        ),
        (
            "<m:scr m:val=\"sans-serif\"/><m:sty m:val=\"bi\"/>",
            Font::SansSerifBoldItalic,
        ),
        (
            "<m:scr m:val=\"monospace\"/><m:sty m:val=\"p\"/>",
            Font::Monospace,
        ),
    ];
    for (properties, font) in cases {
        assert_eq!(
            parse(&document(&run(properties, "R"))).unwrap(),
            math(face('R', Class::Ord, font)),
            "{properties}"
        );
    }
}

#[test]
fn an_alignment_run_is_an_alignment_point() {
    let markup = format!("{}{}", run("", "a"), run("<m:aln m:val=\"1\"/>", ""));
    assert_eq!(
        parse(&document(&markup)).unwrap(),
        math(row([variable('a'), Node::Align]))
    );
}

#[test]
fn a_run_holding_a_space_character_is_a_space_node() {
    let cases = [
        ("\u{2009}", Space::Thin),
        ("\u{2005}", Space::Medium),
        ("\u{2008}", Space::Thick),
        ("\u{2004}", Space::Sixteenth),
        ("\u{2003}", Space::Quad),
        ("\u{2003}\u{2003}", Space::DoubleQuad),
        ("\u{200B}", Space::NegativeThin),
        ("\u{200B}\u{200B}", Space::NegativeMedium),
        ("\u{200B}\u{200B}\u{200B}", Space::NegativeThick),
    ];
    for (text, space) in cases {
        let markup = format!("{}{}{}", run("", "a"), run("", text), run("", "b"));
        assert_eq!(
            parse(&document(&markup)).unwrap(),
            math(row([variable('a'), Node::Space(space), variable('b')]))
        );
    }
}

#[test]
fn a_fraction_carries_its_numerator_denominator_and_rule() {
    let markup =
        "<m:f><m:num><m:r><m:t>1</m:t></m:r></m:num><m:den><m:r><m:t>2</m:t></m:r></m:den></m:f>";
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Frac(Box::new(Frac {
            num: face('1', Class::Ord, Font::Upright),
            den: face('2', Class::Ord, Font::Upright),
            kind: FracKind::Bar,
        })))
    );
    for (value, kind) in [
        ("skw", FracKind::Slash),
        ("lin", FracKind::Linear),
        ("noBar", FracKind::NoBar),
    ] {
        let markup = format!(
            "<m:f><m:fPr><m:type m:val=\"{value}\"/></m:fPr><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f>"
        );
        assert_eq!(
            parse(&document(&markup)).unwrap(),
            math(Node::Frac(Box::new(Frac {
                num: variable('a'),
                den: variable('b'),
                kind
            }))),
            "{value}"
        );
    }
}

#[test]
fn a_radical_index_is_hidden_by_deg_hide() {
    let square = "<m:rad><m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:deg/><m:e><m:r><m:t>x</m:t></m:r></m:e></m:rad>";
    assert_eq!(
        parse(&document(square)).unwrap(),
        math(Node::Sqrt(Box::new(Sqrt::square(variable('x')))))
    );
    let cubed = "<m:rad><m:radPr><m:degHide m:val=\"0\"/></m:radPr><m:deg><m:r><m:t>3</m:t></m:r></m:deg><m:e><m:r><m:t>x</m:t></m:r></m:e></m:rad>";
    assert_eq!(
        parse(&document(cubed)).unwrap(),
        math(Node::Sqrt(Box::new(Sqrt::nth(
            face('3', Class::Ord, Font::Upright),
            variable('x')
        ))))
    );
}

#[test]
fn scripts_attach_to_their_base() {
    let markup = "<m:sSubSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sub><m:r><m:t>1</m:t></m:r></m:sub><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSubSup>";
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Scripts(Box::new(Scripts {
            base: variable('x'),
            sub: Some(face('1', Class::Ord, Font::Upright)),
            sup: Some(face('2', Class::Ord, Font::Upright)),
        })))
    );
    let markup =
        "<m:sSub><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sub><m:r><m:t>i</m:t></m:r></m:sub></m:sSub>";
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Scripts(Box::new(Scripts {
            base: variable('x'),
            sub: Some(variable('i')),
            sup: None
        })))
    );
    let markup =
        "<m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup>";
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Scripts(Box::new(Scripts {
            base: variable('x'),
            sub: None,
            sup: Some(face('2', Class::Ord, Font::Upright)),
        })))
    );
}

#[test]
fn pre_scripts_read_as_ordinary_scripts() {
    let markup = "<m:sPre><m:sub><m:r><m:t>i</m:t></m:r></m:sub><m:sup><m:r><m:t>2</m:t></m:r></m:sup><m:e><m:r><m:t>x</m:t></m:r></m:e></m:sPre>";
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Scripts(Box::new(Scripts {
            base: variable('x'),
            sub: Some(variable('i')),
            sup: Some(face('2', Class::Ord, Font::Upright))
        })))
    );
}

#[test]
fn a_large_operator_becomes_a_big_node_and_keeps_its_operand() {
    let markup = "<m:nary><m:naryPr><m:chr m:val=\"∑\"/><m:limLoc m:val=\"undOvr\"/></m:naryPr><m:sub><m:r><m:t>0</m:t></m:r></m:sub><m:sup><m:r><m:t>n</m:t></m:r></m:sup><m:e><m:r><m:t>x</m:t></m:r></m:e></m:nary>";
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(row([
            Node::Big(Box::new(BigOp {
                base: symbol('∑', Class::Op),
                sub: Some(face('0', Class::Ord, Font::Upright)),
                sup: Some(variable('n')),
                limits: true,
            })),
            variable('x'),
        ]))
    );
}

#[test]
fn a_large_operator_without_limits_keeps_its_scripts_beside_it() {
    let markup = "<m:nary><m:naryPr><m:chr m:val=\"∫\"/></m:naryPr><m:sub><m:r><m:t>a</m:t></m:r></m:sub><m:sup><m:r><m:t>b</m:t></m:r></m:sup><m:e/></m:nary>";
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Big(Box::new(BigOp {
            base: symbol('∫', Class::Op),
            sub: Some(variable('a')),
            sup: Some(variable('b')),
            limits: false,
        })))
    );
}

#[test]
fn delimiters_come_from_beg_chr_and_end_chr() {
    let markup = "<m:d><m:dPr><m:begChr m:val=\"[\"/><m:endChr m:val=\"]\"/></m:dPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:d>";
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Fence(Box::new(Fence {
            open: Delim::Bracket,
            close: Delim::Bracket,
            body: variable('x')
        })))
    );
    let markup = "<m:d><m:dPr><m:begChr m:val=\"\"/><m:endChr m:val=\"\"/></m:dPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:d>";
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Fence(Box::new(Fence {
            open: Delim::None,
            close: Delim::None,
            body: variable('x')
        })))
    );
    let markup = "<m:d><m:e><m:r><m:t>x</m:t></m:r></m:e></m:d>";
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Fence(Box::new(Fence {
            open: Delim::Paren,
            close: Delim::Paren,
            body: variable('x')
        })))
    );
}

#[test]
fn several_arguments_between_delimiters_join_into_one_body() {
    let markup = "<m:d><m:e><m:r><m:t>a</m:t></m:r></m:e><m:e><m:r><m:t>b</m:t></m:r></m:e></m:d>";
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Fence(Box::new(Fence {
            open: Delim::Paren,
            close: Delim::Paren,
            body: row([variable('a'), variable('b')]),
        })))
    );
}

#[test]
fn a_table_between_delimiters_takes_them_as_its_own() {
    let markup = "<m:d><m:e><m:m><m:mr><m:e><m:r><m:t>1</m:t></m:r></m:e></m:mr></m:m></m:e></m:d>";
    let mut matrix = Matrix::empty(1, 1);
    matrix
        .cell_at_mut(0)
        .map(|cell| *cell = face('1', Class::Ord, Font::Upright));
    matrix.delims = Some((Delim::Paren, Delim::Paren));
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Matrix(Box::new(matrix)))
    );
}

#[test]
fn a_table_reads_its_rows_cells_and_column_justification() {
    let markup = "<m:m><m:mPr><m:baseJc m:val=\"center\"/><m:mcs><m:mc><m:mcPr><m:count m:val=\"1\"/><m:mcJc m:val=\"left\"/></m:mcPr></m:mc><m:mc><m:mcPr><m:count m:val=\"1\"/><m:mcJc m:val=\"right\"/></m:mcPr></m:mc></m:mcs></m:mPr><m:mr><m:e><m:r><m:t>a</m:t></m:r></m:e><m:e><m:r><m:t>b</m:t></m:r></m:e></m:mr></m:m>";
    let mut matrix = Matrix::empty(1, 2);
    matrix.cell_at_mut(0).map(|cell| *cell = variable('a'));
    matrix.cell_at_mut(1).map(|cell| *cell = variable('b'));
    matrix.columns = vec![Side::Left, Side::Right];
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Matrix(Box::new(matrix)))
    );
}

#[test]
fn a_column_count_larger_than_the_table_is_capped() {
    // `m:count` used to size a loop directly, so a tiny document could ask for gigabytes; it is
    // capped at the table's own column count.
    let markup = "<m:m><m:mPr><m:mcs><m:mc><m:mcPr><m:count m:val=\"4000000000\"/><m:mcJc m:val=\"right\"/></m:mcPr></m:mc></m:mcs></m:mPr><m:mr><m:e/><m:e/></m:mr></m:m>";
    let mut matrix = Matrix::empty(1, 2);
    matrix.align = Side::Right;
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Matrix(Box::new(matrix)))
    );
}

#[test]
fn a_column_justification_of_every_column_folds_into_the_default() {
    let markup = "<m:m><m:mPr><m:mcs><m:mc><m:mcPr><m:count m:val=\"2\"/><m:mcJc m:val=\"right\"/></m:mcPr></m:mc></m:mcs></m:mPr><m:mr><m:e/><m:e/></m:mr></m:m>";
    let mut matrix = Matrix::empty(1, 2);
    matrix.align = Side::Right;
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Matrix(Box::new(matrix)))
    );
}

#[test]
fn an_equation_array_is_an_aligned_table() {
    let markup =
        "<m:eqArr><m:e><m:r><m:t>a</m:t></m:r></m:e><m:e><m:r><m:t>b</m:t></m:r></m:e></m:eqArr>";
    let mut matrix = Matrix::empty(2, 1);
    matrix.cell_at_mut(0).map(|cell| *cell = variable('a'));
    matrix.cell_at_mut(1).map(|cell| *cell = variable('b'));
    matrix.kind = MatrixKind::Aligned;
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Matrix(Box::new(matrix)))
    );
}

#[test]
fn accents_read_from_their_character_and_position() {
    let cases = [
        (
            "<m:acc><m:accPr><m:chr m:val=\"^\"/></m:accPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:acc>",
            AccentKind::Hat,
            false,
        ),
        (
            "<m:acc><m:accPr><m:chr m:val=\"~\"/><m:pos m:val=\"bot\"/></m:accPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:acc>",
            AccentKind::Tilde,
            true,
        ),
        (
            "<m:bar><m:barPr><m:pos m:val=\"bot\"/></m:barPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:bar>",
            AccentKind::Bar,
            true,
        ),
        (
            "<m:bar><m:e><m:r><m:t>x</m:t></m:r></m:e></m:bar>",
            AccentKind::Bar,
            false,
        ),
        (
            "<m:groupChr><m:groupChrPr><m:chr m:val=\"⏞\"/><m:pos m:val=\"top\"/></m:groupChrPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:groupChr>",
            AccentKind::OverBrace,
            false,
        ),
        (
            "<m:groupChr><m:groupChrPr><m:chr m:val=\"⏟\"/></m:groupChrPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:groupChr>",
            AccentKind::UnderBrace,
            true,
        ),
        (
            "<m:groupChr><m:groupChrPr><m:chr m:val=\"[\"/><m:pos m:val=\"top\"/></m:groupChrPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:groupChr>",
            AccentKind::OverBracket,
            false,
        ),
        // `m:acc` cannot draw a mark under the base, so a mark under it rides in `m:groupChr`.
        (
            "<m:groupChr><m:groupChrPr><m:chr m:val=\"^\"/><m:pos m:val=\"bot\"/></m:groupChrPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:groupChr>",
            AccentKind::Hat,
            true,
        ),
    ];
    for (markup, kind, under) in cases {
        assert_eq!(
            parse(&document(markup)).unwrap(),
            math(Node::Accent(Box::new(Accent {
                kind,
                base: variable('x'),
                under
            }))),
            "{markup}"
        );
    }
}

#[test]
fn an_accent_character_outside_the_model_stays_as_over_or_under_material() {
    let markup =
        "<m:acc><m:accPr><m:chr m:val=\"⌢\"/></m:accPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:acc>";
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Limits(Box::new(Limits {
            base: variable('x'),
            under: None,
            over: Some(symbol('⌢', Class::Ord)),
            align: Side::Center,
        })))
    );
}

#[test]
fn limits_read_from_lim_low_and_lim_upp() {
    let markup = "<m:limUpp><m:e><m:r><m:t>x</m:t></m:r></m:e><m:lim><m:r><m:t>y</m:t></m:r></m:lim></m:limUpp>";
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Limits(Box::new(Limits {
            base: variable('x'),
            under: None,
            over: Some(variable('y')),
            align: Side::Center
        })))
    );
    // Nesting is how Office writes material on both sides of one base.
    let markup = "<m:limLow><m:e><m:limUpp><m:e><m:r><m:t>x</m:t></m:r></m:e><m:lim><m:r><m:t>y</m:t></m:r></m:lim></m:limUpp></m:e><m:lim><m:r><m:t>z</m:t></m:r></m:lim></m:limLow>";
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Limits(Box::new(Limits {
            base: variable('x'),
            under: Some(variable('z')),
            over: Some(variable('y')),
            align: Side::Center,
        })))
    );
}

#[test]
fn a_function_is_a_row_of_its_name_and_argument() {
    let markup = "<m:func><m:fName><m:r><m:rPr><m:sty m:val=\"p\"/></m:rPr><m:t>sin</m:t></m:r></m:fName><m:e><m:r><m:t>x</m:t></m:r></m:e></m:func>";
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(row([
            Node::Text(Box::new(TextRun {
                text: "sin".to_owned(),
                font: Font::Upright
            })),
            variable('x'),
        ]))
    );
}

#[test]
fn boxes_and_phantoms_wrap_their_argument() {
    let boxed = "<m:box><m:e><m:r><m:t>x</m:t></m:r></m:e></m:box>";
    assert_eq!(
        parse(&document(boxed)).unwrap(),
        math(Node::Styled(Box::new(Styled {
            body: variable('x'),
            style: None,
            font: None,
            class: None
        })))
    );
    let bordered = "<m:borderBox><m:e><m:r><m:t>x</m:t></m:r></m:e></m:borderBox>";
    assert_eq!(
        parse(&document(bordered)).unwrap(),
        math(Node::Styled(Box::new(Styled {
            body: variable('x'),
            style: None,
            font: None,
            class: Some(Class::Inner)
        })))
    );
    let phantom = "<m:phant><m:e><m:r><m:t>x</m:t></m:r></m:e></m:phant>";
    assert_eq!(
        parse(&document(phantom)).unwrap(),
        math(Node::Styled(Box::new(Styled {
            body: variable('x'),
            style: None,
            font: None,
            class: None
        })))
    );
}

#[test]
fn argument_properties_are_skipped() {
    let markup = "<m:f><m:num><m:argPr><m:argSz m:val=\"2\"/></m:argPr><m:r><m:t>1</m:t></m:r><m:ctrlPr><m:rPr/></m:ctrlPr></m:num><m:den><m:r><m:t>2</m:t></m:r></m:den></m:f>";
    assert_eq!(
        parse(&document(markup)).unwrap(),
        math(Node::Frac(Box::new(Frac {
            num: face('1', Class::Ord, Font::Upright),
            den: face('2', Class::Ord, Font::Upright),
            kind: FracKind::Bar,
        })))
    );
}

#[test]
fn a_paragraph_reads_its_equations_and_its_justification() {
    let markup = "<m:oMathPara xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">\
        <m:oMathParaPr><m:jc m:val=\"center\"/></m:oMathParaPr>\
        <m:oMath><m:r><m:t>x</m:t></m:r><m:r><m:t>+</m:t></m:r></m:oMath>\
    </m:oMathPara>";
    assert_eq!(
        parse(markup).unwrap(),
        math(row([variable('x'), symbol('+', Class::Bin)]))
    );
}

#[test]
fn several_equations_in_a_paragraph_join_into_one_body() {
    let markup = "<m:oMathPara xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"><m:oMath><m:r><m:t>a</m:t></m:r></m:oMath><m:oMath><m:r><m:t>b</m:t></m:r></m:oMath></m:oMathPara>";
    assert_eq!(
        parse(markup).unwrap(),
        math(row([variable('a'), variable('b')]))
    );
}

#[test]
fn a_prefixed_document_reports_full_size() {
    let markup = "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"><m:r><m:t>y</m:t></m:r></m:oMath>";
    assert_eq!(
        parse(markup).unwrap(),
        Math {
            body: variable('y'),
            style: MathStyle::Full
        }
    );
}

#[test]
fn whitespace_between_elements_is_not_content() {
    let markup = "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">\n  <m:r>\n    <m:t>a</m:t>\n  </m:r>\n  <m:r><m:t>b</m:t></m:r>\n</m:oMath>";
    assert_eq!(
        parse(markup).unwrap(),
        math(row([variable('a'), variable('b')]))
    );
}

#[test]
fn literal_text_keeps_its_escaped_characters() {
    let markup = run("<m:nor m:val=\"1\"/>", "if x &lt; 1 &amp; y &gt; 0");
    assert_eq!(
        parse(&document(&markup)).unwrap(),
        math(Node::Text(Box::new(TextRun {
            text: "if x < 1 & y > 0".to_owned(),
            font: Font::Upright
        })))
    );
}

#[test]
fn a_space_only_text_run_keeps_its_whitespace() {
    let markup = run("<m:nor m:val=\"1\"/>", " ");
    assert_eq!(
        parse(&document(&markup)).unwrap(),
        math(Node::Text(Box::new(TextRun {
            text: " ".to_owned(),
            font: Font::Upright
        })))
    );
}

#[test]
fn a_character_run_takes_its_class_from_the_character() {
    let markup = run("<m:sty m:val=\"b\"/>", "∑");
    assert_eq!(
        parse(&document(&markup)).unwrap(),
        math(face('∑', Class::Op, Font::Bold))
    );
}
