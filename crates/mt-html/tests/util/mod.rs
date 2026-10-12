//! Fixture builders for the export tests: a programmatic realistic .docx container, the same
//! shape `mt-docx`'s own tests use, so the exporter is exercised against hand-written but
//! Word-shaped markup.

#![allow(dead_code)]

use std::io::Write;

use mt_tex::parse as parse_tex;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

/// A 1×1 transparent PNG.
pub const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// The namespaces a Word main part carries.
const NAMESPACES: &str = concat!(
    "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
    "xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\" ",
    "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ",
    "xmlns:v=\"urn:schemas-microsoft-com:vml\" ",
    "xmlns:o=\"urn:schemas-microsoft-com:office:office\" ",
    "xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" ",
    "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" ",
    "xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\""
);

/// An Office Math fragment for an equation given as LaTeX.
pub fn omml(latex: &str, display: bool) -> String {
    let math = parse_tex(latex).expect("the fixture LaTeX parses");
    if display {
        mt_omml::write_paragraph(&math, None)
    } else {
        mt_omml::write(&math)
    }
}

/// An OLE compound file whose equation stream holds the equation given as LaTeX.
pub fn ole_bin(latex: &str) -> Vec<u8> {
    let math = parse_tex(latex).expect("the fixture LaTeX parses");
    mt_mtef::create_ole(&mt_mtef::write(&math)).expect("a compound file")
}

/// The `w:object` markup of an embedded equation object.
pub fn object_run(rel_id: &str, prog_id: &str, preview_rel_id: Option<&str>) -> String {
    let mut out = String::from("<w:object w:dxaOrig=\"1440\" w:dyaOrig=\"288\">");
    if let Some(preview) = preview_rel_id {
        out.push_str(&format!(
            "<v:shapetype id=\"_x0000_t75\" coordsize=\"21600,21600\" o:spt=\"75\" o:preferrelative=\"1\" \
             path=\"m@4@5l@4@11@9@11@9@5xe\" filled=\"f\" stroked=\"f\"/>\
             <v:shape id=\"ole-shape\" type=\"#_x0000_t75\" style=\"width:72pt;height:14.4pt\">\
             <v:imagedata r:id=\"{preview}\" o:title=\"\"/></v:shape>"
        ));
    }
    out.push_str(&format!(
        "<o:OLEObject Type=\"Embed\" ProgID=\"{prog_id}\" ShapeID=\"ole-shape\" \
         DrawAspect=\"Content\" ObjectID=\"_fixture_1\" r:id=\"{rel_id}\"/>"
    ));
    out.push_str("</w:object>");
    out
}

/// An inline picture run.
pub fn image_run(rel_id: &str, name: &str) -> String {
    format!(
        "<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
         <wp:extent cx=\"9525\" cy=\"9525\"/><wp:docPr id=\"1\" name=\"{name}\"/>\
         <a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
         <pic:pic><pic:nvPicPr><pic:cNvPr id=\"1\" name=\"{name}\"/><pic:cNvPicPr/></pic:nvPicPr>\
         <pic:blipFill><a:blip r:embed=\"{rel_id}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
         <pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"9525\" cy=\"9525\"/></a:xfrm>\
         <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData>\
         </a:graphic></wp:inline></w:drawing></w:r>"
    )
}

/// A realistic fixture: a heading with escaping bait, styled runs, inline and display formulas,
/// a table holding a formula, an embedded object and a picture, a superscripted run, and a
/// paragraph carrying an opaque field.
pub fn realistic_docx() -> Vec<u8> {
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <w:document {NAMESPACES}><w:body>\
         <w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr>\
           <w:r><w:t>A heading &amp; a &lt;mark&gt;</w:t></w:r></w:p>\
         <w:p><w:pPr><w:pStyle w:val=\"Heading2\"/></w:pPr><w:r><w:t>Second</w:t></w:r></w:p>\
         <w:p><w:r><w:rPr><w:b/><w:sz w:val=\"28\"/></w:rPr><w:t xml:space=\"preserve\">bold text &amp; more </w:t></w:r> \
           {} <w:r><w:t xml:space=\"preserve\"> tail</w:t></w:r></w:p>\
         <w:p>{}</w:p>\
         <w:tbl><w:tblPr/><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid>\
           <w:tr><w:tc><w:p><w:r><w:t>cell 1</w:t></w:r></w:p></w:tc>\
                 <w:tc><w:p><w:r><w:t>x=</w:t></w:r>{}</w:p></w:tc></w:tr>\
           <w:tr><w:tc><w:p><w:r>{}</w:r></w:p></w:tc>\
                 <w:tc><w:p>{}</w:p></w:tc></w:tr></w:tbl>\
         <w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr>\
           <w:r><w:t xml:space=\"preserve\">a list item </w:t></w:r>\
           <w:r><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr><w:t>2</w:t></w:r></w:p>\
         <w:p><w:r>{}</w:r></w:p>\
         <w:p><w:fldSimple w:instr=\"PAGE\"><w:r><w:t>7</w:t></w:r></w:fldSimple></w:p>\
         <w:p><w:r><w:t>plain paragraph</w:t></w:r></w:p>\
         <w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr></w:body></w:document>",
        omml("a^2+b_2", false),
        omml("\\int_0^1 x\\,dx", true),
        omml("\\sqrt{2}", false),
        object_run("rId5", "Equation.3", Some("rId4")),
        image_run("rId4", "fixture picture"),
        object_run("rId5", "Equation.3", Some("rId4")),
    );
    build_docx(&[
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", root_rels().into_bytes()),
        ("word/document.xml", document.into_bytes()),
        ("word/_rels/document.xml.rels", document_rels().into_bytes()),
        ("word/media/image1.png", PNG.to_vec()),
        ("word/embeddings/oleObject1.bin", ole_bin("\\frac{a}{b}")),
    ])
}

/// The content types part of the fixture.
fn content_types() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
     <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
     <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
     <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
     <Default Extension=\"png\" ContentType=\"image/png\"/>\
     <Default Extension=\"bin\" ContentType=\"application/vnd.openxmlformats-officedocument.oleObject\"/>\
     <Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>"
        .to_owned()
}

/// The package-level relationships of the fixture.
fn root_rels() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
     <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
     <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>"
        .to_owned()
}

/// The main part's relationships of the fixture.
fn document_rels() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
     <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
     <Relationship Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/image1.png\"/>\
     <Relationship Id=\"rId5\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject\" Target=\"embeddings/oleObject1.bin\"/></Relationships>"
        .to_owned()
}

/// Zips parts into a container, deflated like a real one.
pub fn build_docx(parts: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data) in parts {
        writer.start_file(*name, options).expect("start part");
        writer.write_all(data).expect("write part");
    }
    writer.finish().expect("close container").into_inner()
}
