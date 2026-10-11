//! The smoke chain: read a document, rewrite a formula, write it back, and let LibreOffice open
//! the result by converting it — a file it cannot make sense of fails the conversion.

mod util;

use std::path::PathBuf;
use std::process::Command;

use mt_docx::{Position, Replacement};
use mt_tex::parse as parse_tex;
use util::{open, realistic_docx};

/// Runs `soffice --headless --convert-to`, isolated in its own profile, and returns whether the
/// conversion produced an output file.
fn soffice_opens(directory: &std::path::Path, path: &std::path::Path) -> bool {
    let output_dir = directory.join("converted");
    std::fs::create_dir_all(&output_dir).expect("output directory");
    let status = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}/profile", directory.display()))
        .arg("--convert-to")
        .arg("docx:MS Word 2007 XML")
        .arg("--outdir")
        .arg(&output_dir)
        .arg(path)
        .output()
        .expect("soffice runs");
    if !status.status.success() {
        return false;
    }
    let name = path.file_stem().unwrap().to_string_lossy().into_owned() + ".docx";
    output_dir.join(name).exists()
}

fn scratch(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("mt-docx-smoke-{}-{}", std::process::id(), name));
    std::fs::create_dir_all(&directory).expect("scratch directory");
    directory
}

/// The whole chain: read → replace a formula → write back → LibreOffice opens the result.
#[test]
fn an_edited_document_still_opens() {
    let directory = scratch("edited");
    let mut document = open(&realistic_docx());
    let equation = document.equations().into_iter().next().expect("a formula");
    let math = parse_tex("\\frac{\\pi}{2}").unwrap();
    document
        .replace(equation.block, equation.item, Replacement::OMath { math, display: false })
        .expect("replaced");
    // A label run right after the display formula, the 公式编号 shape.
    let display = document
        .equations()
        .into_iter()
        .find(|equation| matches!(equation.kind, mt_docx::EquationKind::Omath { display: true }))
        .expect("display formula");
    document
        .insert_field(
            Position::AfterItem { block: display.block, item: display.item },
            "<w:r xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
             <w:t xml:space=\"preserve\"> (1)</w:t></w:r>",
        )
        .expect("field inserted");

    let path = directory.join("edited.docx");
    std::fs::write(&path, document.into_bytes().expect("written back")).expect("saved");
    assert!(soffice_opens(&directory, &path), "LibreOffice refuses the edited document");
}

/// A document LibreOffice itself produced — namespaces, styles, and satellite parts as a real
/// writer shapes them — round trips and edits through this crate.
#[test]
fn a_libreoffice_document_round_trips() {
    let directory = scratch("lo-produced");
    // A flat ODT converts without needing a zip writer in the test.
    let fodt = directory.join("source.fodt");
    std::fs::write(
        &fodt,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <office:document xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" office:mimetype=\"application/vnd.oasis.opendocument.text\">\
         <office:body><office:text>\
         <text:h text:outline-level=\"1\">Kopf</text:h>\
         <text:p>erster Absatz</text:p>\
         <text:p>zweiter Absatz</text:p>\
         </office:text></office:body></office:document>",
    )
    .expect("fodt written");
    let converted = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}/profile", directory.display()))
        .arg("--convert-to")
        .arg("docx:MS Word 2007 XML")
        .arg("--outdir")
        .arg(&directory)
        .arg(&fodt)
        .output()
        .expect("soffice runs");
    assert!(converted.status.success(), "soffice converted the fixture");
    let produced = directory.join("source.docx");
    assert!(produced.exists(), "soffice produced a document");

    let bytes = std::fs::read(&produced).expect("read");
    let mut document = open(&bytes);
    assert!(document.blocks().len() >= 3);
    // Untouched, every part's content comes back identical. (A LibreOffice archive is written
    // as a stream, so its local headers carry data descriptors this crate's writer normalises —
    // entry content is the guarantee, not whole-file bytes.)
    assert_eq!(util::parts_of(&open(&bytes).into_bytes().unwrap()), util::parts_of(&bytes));
    // Edited — a formula appended at the end of the body — it opens again.
    let math = parse_tex("x+y").unwrap();
    document.insert_omath(Position::BodyEnd, &math, true).expect("inserted");
    let edited = directory.join("edited.docx");
    std::fs::write(&edited, document.into_bytes().expect("written")).expect("saved");
    assert!(soffice_opens(&directory, &edited), "LibreOffice refuses the edited document");
}
