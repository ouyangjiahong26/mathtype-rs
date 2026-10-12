//! Error fixtures: malformed documents and constructs the tree cannot hold must be reported,
//! never guessed at and never a panic.

use mt_omml::{ParseError, parse};

/// A document wrapping the given markup in `<m:oMath>` with the namespace declared.
fn document(markup: &str) -> String {
    format!(
        "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">{markup}</m:oMath>"
    )
}

/// The error the given markup produces.
fn error(markup: &str) -> ParseError {
    let document = document(markup);
    match parse(&document) {
        Ok(tree) => panic!("expected an error for {document}, read {tree:?}"),
        Err(error) => error,
    }
}

#[test]
fn the_root_must_be_an_equation() {
    assert_eq!(
        parse("<m:mrow/>").unwrap_err(),
        ParseError::Root("mrow".to_owned())
    );
    assert_eq!(
        parse("<w:p xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"/>")
            .unwrap_err(),
        ParseError::Root("p".to_owned())
    );
}

#[test]
fn unknown_elements_are_rejected() {
    assert_eq!(
        error("<m:foo/>"),
        ParseError::UnknownElement("foo".to_owned())
    );
    assert!(matches!(
        error("<m:e><m:r><m:t>x</m:t></m:r></m:e>"),
        ParseError::UnknownElement(_)
    ));
    assert!(matches!(
        error("<m:t>x</m:t>"),
        ParseError::UnknownElement(_)
    ));
    assert!(matches!(
        error("<m:math><m:r><m:t>x</m:t></m:r></m:math>"),
        ParseError::UnknownElement(_)
    ));
}

#[test]
fn malformed_xml_is_rejected() {
    for document in [
        "",
        "   ",
        "not xml",
        "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">",
        "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"><m:r><m:t>x</m:t></m:r>",
        "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"><m:r><m:t>x</m:r></m:r></m:oMath>",
        "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"><m:r><m:t>&bogus;</m:t></m:r></m:oMath>",
        "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"><m:r><m:t>a & b</m:t></m:r></m:oMath>",
    ] {
        assert!(
            matches!(parse(document), Err(ParseError::Xml(_))),
            "{document}"
        );
    }
}

#[test]
fn missing_required_arguments_are_rejected() {
    for markup in [
        "<m:f><m:num><m:r><m:t>a</m:t></m:r></m:num></m:f>",
        "<m:f><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f>",
        "<m:sSub><m:sub><m:r><m:t>i</m:t></m:r></m:sub></m:sSub>",
        "<m:acc><m:accPr><m:chr m:val=\"^\"/></m:accPr></m:acc>",
        "<m:bar><m:barPr><m:pos m:val=\"top\"/></m:barPr></m:bar>",
        "<m:limUpp><m:lim><m:r><m:t>y</m:t></m:r></m:lim></m:limUpp>",
        "<m:func><m:fName><m:r><m:t>sin</m:t></m:r></m:fName></m:func>",
        "<m:box><m:boxPr/></m:box>",
        "<m:d><m:dPr><m:begChr m:val=\"(\"/></m:dPr></m:d>",
        "<m:eqArr><m:eqArrPr/></m:eqArr>",
        "<m:groupChr><m:groupChrPr><m:chr m:val=\"{\"/></m:groupChrPr></m:groupChr>",
    ] {
        assert!(
            matches!(error(markup), ParseError::Structure(_)),
            "{markup}"
        );
    }
}

#[test]
fn a_row_of_a_table_needs_a_cell() {
    assert!(matches!(
        error("<m:m><m:mr></m:mr></m:m>"),
        ParseError::Structure(_)
    ));
}

#[test]
fn misplaced_elements_are_rejected() {
    assert!(matches!(
        error("<m:num><m:r><m:t>a</m:t></m:r></m:num>"),
        ParseError::UnknownElement(_)
    ));
    assert!(matches!(
        error("<m:f><m:num><m:r><m:t>a</m:t></m:r></m:num><m:e/></m:f>"),
        ParseError::UnknownElement(_)
    ));
    assert!(matches!(
        error("<m:m><m:e/></m:m>"),
        ParseError::UnknownElement(_)
    ));
    assert!(matches!(
        error("<m:box><m:num><m:r><m:t>a</m:t></m:r></m:num></m:box>"),
        ParseError::UnknownElement(_)
    ));
}

#[test]
fn property_values_outside_the_model_are_rejected() {
    for markup in [
        "<m:r><m:rPr><m:sty m:val=\"q\"/></m:rPr><m:t>x</m:t></m:r>".to_owned(),
        "<m:r><m:rPr><m:scr m:val=\"comic\"/></m:rPr><m:t>x</m:t></m:r>".to_owned(),
        "<m:f><m:fPr><m:type m:val=\"diagonal\"/></m:fPr><m:num/><m:den/></m:f>".to_owned(),
        "<m:nary><m:naryPr><m:limLoc m:val=\"sideways\"/></m:naryPr><m:sub/><m:sup/><m:e/></m:nary>".to_owned(),
        "<m:acc><m:accPr><m:chr m:val=\"^\"/><m:pos m:val=\"left\"/></m:accPr><m:e/></m:acc>".to_owned(),
        "<m:bar><m:barPr><m:pos m:val=\"left\"/></m:barPr><m:e/></m:bar>".to_owned(),
        "<m:groupChr><m:groupChrPr><m:chr m:val=\"{\"/><m:pos m:val=\"left\"/></m:groupChrPr><m:e/></m:groupChr>".to_owned(),
        "<m:nary><m:naryPr><m:chr m:val=\"ab\"/></m:naryPr><m:sub/><m:sup/><m:e/></m:nary>".to_owned(),
        "<m:d><m:dPr><m:begChr m:val=\"ab\"/></m:dPr><m:e/></m:d>".to_owned(),
        "<m:m><m:mPr><m:mcs><m:mc><m:mcPr><m:count m:val=\"many\"/><m:mcJc m:val=\"left\"/></m:mcPr></m:mc></m:mcs></m:mPr><m:mr><m:e/></m:mr></m:m>".to_owned(),
        "<m:m><m:mPr><m:mcs><m:mc><m:mcPr><m:count m:val=\"1\"/><m:mcJc m:val=\"middle\"/></m:mcPr></m:mc></m:mcs></m:mPr><m:mr><m:e/></m:mr></m:m>".to_owned(),
    ] {
        assert!(
            matches!(parse(&document(&markup)), Err(ParseError::InvalidAttribute { .. })),
            "{markup}: {:?}",
            parse(&document(&markup))
        );
    }
}

#[test]
fn properties_that_the_model_drops_are_accepted() {
    for markup in [
        "<m:r><m:rPr><m:lit m:val=\"1\"/><m:brk/><m:argSz m:val=\"2\"/></m:rPr><m:t>x</m:t></m:r>".to_owned(),
        "<m:m><m:mPr><m:baseJc m:val=\"top\"/><m:plcHide m:val=\"1\"/><m:rSp m:val=\"3\"/><m:cSp m:val=\"3\"/></m:mPr><m:mr><m:e/></m:mr></m:m>".to_owned(),
        "<m:box><m:boxPr><m:opEmu m:val=\"1\"/><m:noBreak m:val=\"1\"/></m:boxPr><m:e/></m:box>".to_owned(),
        "<m:borderBox><m:borderBoxPr><m:hideTop m:val=\"1\"/></m:borderBoxPr><m:e/></m:borderBox>".to_owned(),
        "<m:f><m:fPr><m:ctrlPr/></m:fPr><m:num/><m:den/></m:f>".to_owned(),
        "<m:d><m:dPr><m:begChr m:val=\"(\"/><m:sepChr m:val=\"|\"/><m:endChr m:val=\")\"/><m:grow m:val=\"1\"/></m:dPr><m:e><m:r><m:t>a</m:t></m:r></m:e><m:e><m:r><m:t>b</m:t></m:r></m:e></m:d>".to_owned(),
        "<m:nary><m:naryPr><m:chr m:val=\"∑\"/><m:grow m:val=\"1\"/><m:subHide m:val=\"1\"/><m:supHide m:val=\"1\"/></m:naryPr><m:sub/><m:sup/><m:e/></m:nary>".to_owned(),
        "<m:acc><m:accPr><m:chr m:val=\"^\"/><m:ctrlPr/></m:accPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:acc>".to_owned(),
        "<m:rad><m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:rad>".to_owned(),
    ] {
        assert!(parse(&document(&markup)).is_ok(), "{markup}: {:?}", parse(&document(&markup)));
    }
}

#[test]
fn elements_that_nest_too_deeply_are_rejected() {
    // The reader walks the tree with recursion, so a document this deep has to be turned away
    // before the walk starts: otherwise it overflows the stack, which aborts the process.
    let nested = |levels: usize| {
        let markup = format!(
            "{}<m:r><m:t>x</m:t></m:r>{}",
            "<m:box><m:e>".repeat(levels),
            "</m:e></m:box>".repeat(levels)
        );
        document(&markup)
    };
    assert_eq!(parse(&nested(200)).unwrap_err(), ParseError::Nesting);
    // Just inside the documented limit is still read.
    assert!(parse(&nested(40)).is_ok());
}

#[test]
fn an_error_never_panics_on_a_prefix_of_a_valid_document() {
    let document = document(
        "<m:r><m:t>x</m:t></m:r><m:f><m:num><m:r><m:t>1</m:t></m:r></m:num><m:den><m:r><m:t>2</m:t></m:r></m:den></m:f>",
    );
    for end in 0..=document.len() {
        if !document.is_char_boundary(end) {
            continue;
        }
        let _ = parse(&document[..end]);
    }
}
