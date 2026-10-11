//! Error fixtures: malformed documents and constructs the tree cannot hold must be reported,
//! never guessed at and never a panic.

use mt_mathml::{parse, ParseError};

/// The error `document` produces.
fn error(document: &str) -> ParseError {
    match parse(document) {
        Ok(tree) => panic!("expected an error for {document}, read {tree:?}"),
        Err(error) => error,
    }
}

#[test]
fn the_root_must_be_math() {
    assert_eq!(error("<mrow><mi>x</mi></mrow>"), ParseError::Root("mrow".to_owned()));
    assert_eq!(error("<mi>x</mi>"), ParseError::Root("mi".to_owned()));
}

#[test]
fn unknown_elements_are_rejected() {
    assert_eq!(error("<math><mfoo>x</mfoo></math>"), ParseError::UnknownElement("mfoo".to_owned()));
    assert_eq!(error("<math><mi>x</mi><munderfoo/></math>"), ParseError::UnknownElement("munderfoo".to_owned()));
    assert!(matches!(error("<math><mtr><mtd><mi>x</mi></mtd></mtr></math>"), ParseError::UnknownElement(_)));
    assert!(matches!(error("<math><mtd><mi>x</mi></mtd></math>"), ParseError::UnknownElement(_)));
}

#[test]
fn malformed_xml_is_rejected() {
    for document in [
        "",
        "   ",
        "not xml",
        "<math>",
        "<math><mi>x</mi>",
        "<math><mi>x</math>",
        "<math><mrow><mi>x</mi></mrow>",
        "<math><mi>x</mi></math><math/>",
        "<math><mi>x</mi></math>trailing",
        "<math><mi>x</mi></math<!-- unterminated",
        "<math><mi x=1></mi></math>",
    ] {
        assert!(matches!(error(document), ParseError::Xml(_)), "{document}");
    }
}

#[test]
fn references_that_do_not_resolve_are_rejected() {
    assert!(matches!(error("<math><mtext>&bogus;</mtext></math>"), ParseError::Xml(_)));
    assert!(matches!(error("<math><mtext>&#xZZ;</mtext></math>"), ParseError::Xml(_)));
    assert!(matches!(error("<math><mtext>&#1114112;</mtext></math>"), ParseError::Xml(_)));
    assert!(matches!(error("<math><mtext>a & b</mtext></math>"), ParseError::Xml(_)));
}

#[test]
fn elements_with_the_wrong_number_of_children_are_rejected() {
    for document in [
        "<math><mfrac><mi>a</mi></mfrac></math>",
        "<math><mfrac><mi>a</mi><mi>b</mi><mi>c</mi></mfrac></math>",
        "<math><mfrac/></math>",
        "<math><msqrt/></math>",
        "<math><msqrt><mi>a</mi><mi>b</mi></msqrt></math>",
        "<math><mroot><mi>a</mi></mroot></math>",
        "<math><msub><mi>a</mi></msub></math>",
        "<math><msubsup><mi>a</mi><mi>b</mi></msubsup></math>",
        "<math><munder><mi>a</mi></munder></math>",
        "<math><munderover><mi>a</mi><mi>b</mi></munderover></math>",
    ] {
        assert!(matches!(error(document), ParseError::Structure(_)), "{document}");
    }
}

#[test]
fn table_parts_out_of_place_are_rejected() {
    assert!(matches!(error("<math><mtable><mrow/></mtable></math>"), ParseError::Structure(_)));
    assert!(matches!(error("<math><mtable><mtr><mrow/></mtr></mtable></math>"), ParseError::Structure(_)));
    assert!(matches!(error("<math><mtable><mtd/></mtable></math>"), ParseError::Structure(_)));
}

#[test]
fn attribute_values_outside_the_model_are_rejected() {
    let documents = [
        "<math><mi mathvariant=\"sparkly\">x</mi></math>",
        "<math><mstyle mathvariant=\"sparkly\"><mi>x</mi></mstyle></math>",
        "<math><mspace width=\"2pt\"/></math>",
        "<math><mspace/></math>",
        "<math><mspace width=\"0.5em\"/></math>",
        "<math><mspace width=\"veryverythinmathspace\"/></math>",
        "<math><mtable columnalign=\"middle\"><mtr><mtd/></mtr></mtable></math>",
        "<math><munder align=\"up\"><mi>a</mi><mi>b</mi></munder></math>",
        "<math><mstyle scriptlevel=\"half\"><mi>x</mi></mstyle></math>",
        "<math><mfenced open=\"ab\"><mi>x</mi></mfenced></math>",
    ];
    for document in documents {
        assert!(
            matches!(error(document), ParseError::InvalidAttribute { .. }),
            "{document}: {:?}",
            parse(document)
        );
    }
}

#[test]
fn attributes_that_the_model_drops_are_accepted() {
    // `numalign`, `stretchy` and `separators` have no field in the tree; they are read and
    // ignored rather than treated as errors.
    for document in [
        "<math><mfrac numalign=\"left\" denalign=\"right\"><mi>a</mi><mi>b</mi></mfrac></math>",
        "<math><mo stretchy=\"true\">∑</mo></math>",
        "<math><mo fence=\"true\" separators=\"true\">(</mo></math>",
        "<math><mspace width=\"thinmathspace\" height=\"1em\"/></math>",
        "<math display=\"block\"><mi>x</mi></math>",
    ] {
        assert!(parse(document).is_ok(), "{document}: {:?}", parse(document));
    }
}

#[test]
fn an_error_never_panics_on_a_prefix_of_a_valid_document() {
    let document = "<math display=\"block\"><mrow><mi>x</mi><mo>=</mo><mfrac><mn>1</mn><mn>2</mn></mfrac></mrow></math>";
    for end in 0..=document.len() {
        if !document.is_char_boundary(end) {
            continue;
        }
        let _ = parse(&document[..end]);
    }
}
