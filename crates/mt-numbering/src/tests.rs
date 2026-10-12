//! Field spelling, document round-trip, and persistence coverage for #11.

use std::io::Write;

use crate::*;
use mt_core::Math;
use mt_docx::{Document, Position};
use quick_xml::Reader;

// ------------------------------------------------------------------ fixtures

/// The LaTeX sources of the fixture's equations; five of them.
fn sources() -> Vec<&'static str> {
    vec!["x^2", "\\frac{a}{b}", "\\sqrt{2}", "e^{i\\pi}+1", "a+b"]
}

fn math(source: &str) -> Math {
    mt_tex::parse(source).unwrap()
}

/// Builds a .docx container holding `count` Office Math paragraphs.
fn fixture(count: usize) -> Vec<u8> {
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
         xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:body>",
    );
    for source in sources().into_iter().take(count) {
        xml.push_str("<w:p>");
        xml.push_str(&mt_omml::write(&math(source)));
        xml.push_str("</w:p>");
    }
    xml.push_str("</w:body></w:document>");
    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    writer.start_file("word/document.xml", options).unwrap();
    writer.write_all(xml.as_bytes()).unwrap();
    writer.finish().unwrap().into_inner()
}

/// Re-parses a fragment as XML: names must balance and entities resolve.
fn well_formed(fragment: &str) {
    let wrapped = format!(
        "<root xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
         xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">{fragment}</root>"
    );
    let mut reader = Reader::from_str(&wrapped);
    reader.config_mut().check_end_names = true;
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(_) => {}
            Err(error) => panic!("fragment is not well-formed: {error}: {fragment}"),
        }
    }
}

/// The document re-opened from its own bytes: the "close and reopen" of the issue, with the
/// save/load of the container standing in for the host application.
fn reopened(document: Document) -> Document {
    Document::open(&document.into_bytes().unwrap()).unwrap()
}

// ------------------------------------------------------------------ field XML

#[test]
fn plain_numbering_is_one_arabic_seq_inside_the_macrobutton() {
    let runs = numbering_field_runs(NumberStyle::Plain);
    assert!(
        runs.contains(" MACROBUTTON MTPlaceRef "),
        "the wrapper: {runs}"
    );
    assert!(runs.contains(" SEQ MTEqn \\h "), "the hidden index: {runs}");
    assert!(
        runs.contains(" SEQ MTEqn \\* ARABIC "),
        "the visible number: {runs}"
    );
    assert!(!runs.contains("MTChap") && !runs.contains("MTSec"));
    well_formed(&runs);
}

#[test]
fn chapter_numbering_reads_the_chapter_counter_without_incrementing() {
    let runs = numbering_field_runs(NumberStyle::Chapter);
    assert!(runs.contains(" SEQ MTChap \\c \\* ARABIC "), "{runs}");
    assert!(runs.contains(" SEQ MTEqn \\* ARABIC "), "{runs}");
    assert!(!runs.contains("MTSec"));
    well_formed(&runs);
}

#[test]
fn section_numbering_is_the_full_triple_of_the_issue() {
    let runs = numbering_field_runs(NumberStyle::Section);
    assert!(runs.contains(" SEQ MTChap \\c \\* ARABIC "), "{runs}");
    assert!(runs.contains(" SEQ MTSec \\c \\* ARABIC "), "{runs}");
    let equation = runs.matches(" SEQ MTEqn ").count();
    assert_eq!(
        equation, 2,
        "the hidden index and the visible number: {runs}"
    );
    well_formed(&runs);
}

#[test]
fn paragraph_numbering_restarts_at_heading_one() {
    let runs = numbering_field_runs(NumberStyle::Paragraph);
    assert!(runs.contains(" SEQ MTEqn \\s 1 \\* ARABIC "), "{runs}");
    well_formed(&runs);
}

#[test]
fn the_simple_spelling_covers_single_seq_styles_only() {
    let plain = numbering_field_simple(NumberStyle::Plain).unwrap();
    assert!(
        plain.starts_with("<w:fldSimple w:instr=\" SEQ MTEqn \\* ARABIC \">"),
        "{plain}"
    );
    assert!(plain.contains("(1)"));
    well_formed(&plain);
    let paragraph = numbering_field_simple(NumberStyle::Paragraph).unwrap();
    assert!(paragraph.contains(" SEQ MTEqn \\s 1 \\* ARABIC "));
    assert!(numbering_field_simple(NumberStyle::Chapter).is_none());
    assert!(numbering_field_simple(NumberStyle::Section).is_none());
}

#[test]
fn both_sides_place_the_number_against_the_tab() {
    let equation = "<m:oMath xmlns:m=\"urn:m\"/>";
    let right = numbered_inline(equation, NumberStyle::Plain, Side::Right, "_RefMTEq1", 1);
    let tab = right.find("<w:tab/>").unwrap();
    let number = right.find("MACROBUTTON").unwrap();
    assert!(
        tab < number,
        "right side: the tab comes between equation and number: {right}"
    );
    let left = numbered_inline(equation, NumberStyle::Plain, Side::Left, "_RefMTEq1", 1);
    let tab = left.find("<w:tab/>").unwrap();
    let number = left.find("MACROBUTTON").unwrap();
    assert!(
        number < tab,
        "left side: the number comes before the tab: {left}"
    );
    let paragraph = numbered_paragraph(equation, NumberStyle::Plain, Side::Right, "_RefMTEq1", 1);
    assert!(
        paragraph.contains("<w:tab w:val=\"right\" w:pos=\"8296\"/>"),
        "{paragraph}"
    );
    let left_paragraph =
        numbered_paragraph_standalone(equation, NumberStyle::Plain, Side::Left, "_RefMTEq1", 1);
    assert!(
        left_paragraph.contains("<w:tab w:val=\"left\" w:pos=\"0\"/>"),
        "{left_paragraph}"
    );
    well_formed(&paragraph);
    well_formed(&left_paragraph);
}

#[test]
fn chapter_breaks_reset_the_counters_the_break_sets() {
    let paragraph = break_paragraph(BreakKind::Chapter, 2);
    assert!(paragraph.contains(" SEQ MTChap \\r 2 "), "{paragraph}");
    assert!(paragraph.contains(" SEQ MTEqn \\r 0 "), "{paragraph}");
    assert!(
        paragraph.contains("<w:vanish/>"),
        "the reset runs are hidden: {paragraph}"
    );
    let section = break_paragraph(BreakKind::Section, 3);
    assert!(section.contains(" SEQ MTSec \\r 3 "), "{section}");
    assert!(section.contains(" SEQ MTEqn \\r 0 "));
    well_formed(&paragraph);
    well_formed(&section);
    well_formed(&break_paragraph_standalone(BreakKind::Chapter, 1));
}

#[test]
fn a_reference_is_a_hyperlinked_ref_field_on_the_bookmark() {
    let runs = reference_runs("_RefMTEq7", 7);
    assert!(runs.contains(" REF _RefMTEq7 \\h "), "{runs}");
    assert!(
        runs.contains("(7)"),
        "the cached text shows the number: {runs}"
    );
    well_formed(&runs);
}

#[test]
fn the_bookmark_wraps_what_it_marks() {
    let runs = bookmark_runs("_RefMTEq1", 3, "inner");
    assert_eq!(
        runs,
        "<w:bookmarkStart w:id=\"3\" w:name=\"_RefMTEq1\"/>inner<w:bookmarkEnd w:id=\"3\"/>"
    );
}

// ------------------------------------------------------------------ end to end

#[test]
fn five_inserted_numbers_enumerate_back_after_reopening() {
    let mut document = Document::open(&fixture(0)).unwrap();
    for source in sources() {
        insert_numbered(
            &mut document,
            Position::BodyEnd,
            &math(source),
            NumberStyle::Plain,
            Side::Right,
        )
        .unwrap();
    }
    let document = reopened(document);
    let numbered = enumerate(&document);
    assert_eq!(numbered.len(), 5, "{numbered:?}");
    assert_eq!(
        numbered
            .iter()
            .map(|found| found.number)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4, 5]
    );
    for found in &numbered {
        assert!(found.bookmark.starts_with("_RefMTEq"), "{found:?}");
    }
    let xml = document.document_xml();
    assert_eq!(xml.matches("MACROBUTTON MTPlaceRef").count(), 5);
}

#[test]
fn numbering_existing_equations_appends_fields_that_survive_reopening() {
    let mut document = Document::open(&fixture(5)).unwrap();
    let count = number_existing(&mut document, NumberStyle::Chapter, Side::Right, None).unwrap();
    assert_eq!(count, 5);
    let document = reopened(document);
    let numbered = enumerate(&document);
    assert_eq!(numbered.len(), 5);
    // The numbers join the equations' own paragraphs, one per paragraph of the fixture.
    let blocks: Vec<_> = numbered.iter().map(|found| found.block).collect();
    assert_eq!(blocks, [0, 1, 2, 3, 4], "each in its equation's paragraph");
    assert!(
        document
            .document_xml()
            .contains("<w:tab w:val=\"right\" w:pos=\"8296\"/>")
    );
}

#[test]
fn numbering_one_equation_leaves_the_rest_alone() {
    let mut document = Document::open(&fixture(5)).unwrap();
    let count = number_existing(&mut document, NumberStyle::Plain, Side::Right, Some(2)).unwrap();
    assert_eq!(count, 1);
    let numbered = enumerate(&document);
    assert_eq!(numbered.len(), 1);
}

#[test]
fn deleting_a_number_resequences_the_rest() {
    let mut document = Document::open(&fixture(0)).unwrap();
    for source in &sources() {
        insert_numbered(
            &mut document,
            Position::BodyEnd,
            &math(source),
            NumberStyle::Plain,
            Side::Right,
        )
        .unwrap();
    }
    // Delete the third number: its field is the Other item that holds the bookmarkStart.
    let numbered = enumerate(&document);
    let target = &numbered[2];
    let xml = document.document_xml();
    let marker = format!("w:name=\"{}\"", target.bookmark);
    let tag = xml.find(&marker).unwrap();
    let start = xml[..tag].rfind("<w:bookmarkStart").unwrap();
    let id = xml[start..tag]
        .split("w:id=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned();
    let end_tag = xml[start..]
        .find(&format!("w:bookmarkEnd w:id=\"{id}\""))
        .unwrap()
        + start;
    let end = xml[end_tag..].find('/').unwrap() + end_tag + 2;
    document.splice_range(start..end, "").unwrap();

    let document = reopened(document);
    let numbered = enumerate(&document);
    assert_eq!(numbered.len(), 4);
    assert_eq!(
        numbered
            .iter()
            .map(|found| found.number)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4],
        "the numbers resequence in document order"
    );

    // A number added afterwards takes the next free bookmark, not a deleted one's slot.
    let mut document = document;
    insert_numbered(
        &mut document,
        Position::BodyEnd,
        &math("z"),
        NumberStyle::Plain,
        Side::Right,
    )
    .unwrap();
    let numbered = enumerate(&document);
    assert_eq!(numbered.len(), 5);
    assert_eq!(numbered[4].bookmark, "_RefMTEq6");
}

#[test]
fn adding_an_equation_after_numbers_extends_the_sequence() {
    let mut document = Document::open(&fixture(0)).unwrap();
    for source in &sources()[..3] {
        insert_numbered(
            &mut document,
            Position::BodyEnd,
            &math(source),
            NumberStyle::Plain,
            Side::Right,
        )
        .unwrap();
    }
    let mut document = reopened(document);
    insert_numbered(
        &mut document,
        Position::BodyEnd,
        &math("q"),
        NumberStyle::Plain,
        Side::Right,
    )
    .unwrap();
    assert_eq!(enumerate(&document).len(), 4);
    assert_eq!(enumerate(&document)[3].number, 4);
}

#[test]
fn breaks_land_between_the_numbers_and_carry_their_index() {
    let mut document = Document::open(&fixture(0)).unwrap();
    insert_numbered(
        &mut document,
        Position::BodyEnd,
        &math("a"),
        NumberStyle::Chapter,
        Side::Right,
    )
    .unwrap();
    insert_numbered(
        &mut document,
        Position::BodyEnd,
        &math("b"),
        NumberStyle::Chapter,
        Side::Right,
    )
    .unwrap();
    let first = insert_break(&mut document, BreakKind::Chapter).unwrap();
    assert_eq!(first, 1);
    insert_numbered(
        &mut document,
        Position::BodyEnd,
        &math("c"),
        NumberStyle::Chapter,
        Side::Right,
    )
    .unwrap();
    let second = insert_break(&mut document, BreakKind::Chapter).unwrap();
    assert_eq!(second, 2, "the second break counts the first");
    let section = insert_break(&mut document, BreakKind::Section).unwrap();
    assert_eq!(section, 1, "section breaks count separately");

    let document = reopened(document);
    let xml = document.document_xml();
    // The resets sit after the second number and before the third: chapter 2 restarts there.
    let second_number = xml.match_indices("MACROBUTTON").nth(1).unwrap().0;
    let reset = xml.find(" SEQ MTChap \\r 1 ").unwrap();
    let third_number = xml.match_indices("MACROBUTTON").nth(2).unwrap().0;
    assert!(second_number < reset && reset < third_number);
    assert!(xml.contains(" SEQ MTChap \\r 2 "));
    assert!(xml.contains(" SEQ MTSec \\r 1 "));
}

#[test]
fn a_cross_reference_points_at_the_bookmark_of_its_target() {
    let mut document = Document::open(&fixture(0)).unwrap();
    for source in &sources()[..3] {
        insert_numbered(
            &mut document,
            Position::BodyEnd,
            &math(source),
            NumberStyle::Plain,
            Side::Right,
        )
        .unwrap();
    }
    let bookmark =
        insert_reference(&mut document, Position::BodyEnd, ReferenceTarget::Number(2)).unwrap();
    assert_eq!(bookmark, "_RefMTEq2");
    let document = reopened(document);
    let xml = document.document_xml();
    assert!(xml.contains(" REF _RefMTEq2 \\h "));
    assert!(
        xml.contains("w:name=\"_RefMTEq2\""),
        "the bookmark is still there"
    );

    let mut document = document;
    let latest =
        insert_reference(&mut document, Position::BodyEnd, ReferenceTarget::Latest).unwrap();
    assert_eq!(latest, "_RefMTEq3");
}

#[test]
fn a_reference_without_a_target_is_refused() {
    let document = Document::open(&fixture(2)).unwrap();
    let mut document = document;
    assert!(insert_reference(&mut document, Position::BodyEnd, ReferenceTarget::Latest).is_err());
}

// ------------------------------------------------------------------ settings

#[test]
fn settings_round_trip_through_save_and_reopen() {
    let mut document = Document::open(&fixture(3)).unwrap();
    assert_eq!(read_settings(&document), (NumberStyle::Plain, Side::Right));
    write_settings(&mut document, NumberStyle::Chapter, Side::Left).unwrap();
    let document = reopened(document);
    assert_eq!(read_settings(&document), (NumberStyle::Chapter, Side::Left));

    // Updating the marker replaces it rather than adding a second one.
    let mut document = document;
    write_settings(&mut document, NumberStyle::Section, Side::Right).unwrap();
    let document = reopened(document);
    assert_eq!(
        read_settings(&document),
        (NumberStyle::Section, Side::Right)
    );
    assert_eq!(document.document_xml().matches("MTNumberStyle=").count(), 1);
}

#[test]
fn settings_land_in_the_first_paragraph_and_never_break_it() {
    let mut document = Document::open(&fixture(0)).unwrap();
    write_settings(&mut document, NumberStyle::Paragraph, Side::Left).unwrap();
    insert_numbered(
        &mut document,
        Position::BodyEnd,
        &math("a"),
        NumberStyle::Paragraph,
        Side::Right,
    )
    .unwrap();
    let document = reopened(document);
    assert_eq!(
        read_settings(&document),
        (NumberStyle::Paragraph, Side::Left)
    );
    assert_eq!(enumerate(&document).len(), 1);
}

// ------------------------------------------------------------------ parsing edge

#[test]
fn numbering_a_paragraph_that_already_has_properties_keeps_them() {
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
         xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"><w:body>\
         <w:p><w:pPr><w:pStyle w:val=\"Normal\"/></w:pPr>",
    );
    xml.push_str(&mt_omml::write(&math("x")));
    xml.push_str("</w:p></w:body></w:document>");
    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    writer
        .start_file("word/document.xml", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(xml.as_bytes()).unwrap();
    let bytes = writer.finish().unwrap().into_inner();

    let mut document = Document::open(&bytes).unwrap();
    number_existing(&mut document, NumberStyle::Plain, Side::Right, None).unwrap();
    let xml = document.document_xml();
    assert!(
        xml.contains("<w:pPr><w:tabs>"),
        "the tab stop joins the existing properties: {xml}"
    );
    assert!(
        xml.contains("<w:pStyle w:val=\"Normal\"/>"),
        "the style survives: {xml}"
    );
    assert_eq!(enumerate(&document).len(), 1);
    // And the whole document still re-opens, which re-parses every spliced byte.
    let _ = reopened(document);
}

#[test]
fn style_and_side_words_parse() {
    assert_eq!(NumberStyle::parse("chapter"), Some(NumberStyle::Chapter));
    assert_eq!(NumberStyle::parse("Section"), Some(NumberStyle::Section));
    assert_eq!(
        NumberStyle::parse("paragraph"),
        Some(NumberStyle::Paragraph)
    );
    assert_eq!(NumberStyle::parse("plain"), Some(NumberStyle::Plain));
    assert_eq!(NumberStyle::parse("roman"), None);
    assert_eq!(Side::parse("left"), Some(Side::Left));
    assert_eq!(Side::parse("RIGHT"), Some(Side::Right));
    assert_eq!(Side::parse("middle"), None);
}

#[test]
fn replacing_a_number_with_text_leaves_the_rest_numbered() {
    let mut document = Document::open(&fixture(0)).unwrap();
    for source in &sources()[..3] {
        insert_numbered(
            &mut document,
            Position::BodyEnd,
            &math(source),
            NumberStyle::Plain,
            Side::Right,
        )
        .unwrap();
    }
    // The number spans its bookmark; replace the whole bookmarked span with plain text.
    let numbered = enumerate(&document);
    let xml = document.document_xml();
    let marker = format!("w:name=\"{}\"", numbered[1].bookmark);
    let tag = xml.find(&marker).unwrap();
    let start = xml[..tag].rfind("<w:bookmarkStart").unwrap();
    let id = xml[start..tag]
        .split("w:id=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned();
    let end_tag = xml[start..]
        .find(&format!("w:bookmarkEnd w:id=\"{id}\""))
        .unwrap()
        + start;
    let end = xml[end_tag..].find('/').unwrap() + end_tag + 2;
    document
        .splice_range(start..end, "<w:r><w:t>(固定)</w:t></w:r>")
        .unwrap();
    let numbered = enumerate(&document);
    assert_eq!(
        numbered.len(),
        2,
        "the replaced field no longer counts: {numbered:?}"
    );
    assert!(document.document_xml().contains("(固定)"));
}
