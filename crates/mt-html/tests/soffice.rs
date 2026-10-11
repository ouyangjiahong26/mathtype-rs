//! A real writer's markup: the realistic fixture is rewritten by LibreOffice (`--convert-to
//! docx` loads it into the Writer engine and writes it back out in its own dialect), then
//! exported, so the generator meets markup no hand-written fixture shaped.

mod util;

use std::process::Command;

use mt_docx::Document;
use mt_html::{Options, export};

/// Rewrites the fixture through LibreOffice, isolated in a throwaway profile, and returns the
/// rewritten container's bytes.
fn soffice_rewrite(directory: &std::path::Path, bytes: &[u8]) -> Vec<u8> {
    let source = directory.join("source.docx");
    std::fs::write(&source, bytes).expect("docx written");
    let converted = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", directory.join("profile").display()))
        .arg("--convert-to")
        .arg("docx")
        .arg("--outdir")
        .arg(directory.join("out"))
        .arg(&source)
        .output()
        .expect("soffice runs");
    assert!(converted.status.success(), "soffice rewrote the fixture");
    let produced = directory.join("out").join("source.docx");
    assert!(produced.exists(), "soffice produced a document");
    std::fs::read(&produced).expect("the rewritten document is readable")
}

#[test]
fn a_libreoffice_document_exports_to_mathml() {
    let directory = std::env::temp_dir().join(format!("mt-html-soffice-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("the scratch directory is created");

    let bytes = soffice_rewrite(&directory, &util::realistic_docx());
    let document = Document::open(&bytes).expect("the rewritten document opens");
    let html = export(&document, &Options::default()).html;

    assert!(html.matches("<math").count() >= 3, "the formulas survive the rewrite: {html}");
    assert!(html.contains("<table>"), "{html}");
    assert!(html.contains("cell 1"), "{html}");
    assert!(html.contains("plain paragraph"), "{html}");
    assert!(!html.contains("m:math"), "{html}");

    let _ = std::fs::remove_dir_all(&directory);
}
