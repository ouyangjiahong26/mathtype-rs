//! Fixture documents and pipeline coverage for #13.
//!
//! The fixture is a real container: a zip holding `word/document.xml` with 22 paragraphs of Office
//! Math plus two paragraphs of broken OLE objects (a real `w:object` run whose data part is
//! garbage), and the relationship part the objects' `r:id`s hang from.

use std::io::Write;

use crate::{Entry, Failure, Mode, Options, PlanStats, Progress, Report, TargetFormat, plan, run};
use mt_docx::Document;

/// The LaTeX sources of the fixture's Office Math formulas; 22 of them.
fn formulas() -> Vec<&'static str> {
    (1..=22)
        .map(|index| match index % 4 {
            0 => "e^{i\\pi}+1",
            1 => "x^2",
            2 => "\\frac{a}{b}",
            _ => "\\sqrt{2}",
        })
        .collect()
}

/// Builds a .docx container: `count` Office Math paragraphs plus two broken OLE object paragraphs.
fn fixture(count: usize) -> Vec<u8> {
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
         xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
         xmlns:o=\"urn:schemas-microsoft-com:office:office\"><w:body>",
    );
    for source in formulas().into_iter().take(count) {
        let math = mt_tex::parse(source).unwrap();
        let markup = mt_omml::write(&math);
        xml.push_str("<w:p>");
        xml.push_str(&markup);
        xml.push_str("</w:p>");
    }
    for id in ["rIdBad1", "rIdBad2"] {
        xml.push_str(&format!(
            "<w:p><w:r><w:object><o:OLEObject Type=\"Embed\" ProgID=\"Equation.3\" \
             ShapeID=\"_s{id}\" DrawAspect=\"Content\" ObjectID=\"_o{id}\" r:id=\"{id}\"/></w:object></w:r></w:p>"
        ));
    }
    xml.push_str("</w:body></w:document>");

    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
        <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
        <Relationship Id=\"rIdBad1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject\" Target=\"embeddings/bad1.bin\"/>\
        <Relationship Id=\"rIdBad2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject\" Target=\"embeddings/bad2.bin\"/>\
        </Relationships>";

    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    writer.start_file("word/document.xml", options).unwrap();
    writer.write_all(xml.as_bytes()).unwrap();
    writer
        .start_file("word/_rels/document.xml.rels", options)
        .unwrap();
    writer.write_all(rels.as_bytes()).unwrap();
    writer
        .start_file("word/embeddings/bad1.bin", options)
        .unwrap();
    writer.write_all(b"this is not a compound file").unwrap();
    writer
        .start_file("word/embeddings/bad2.bin", options)
        .unwrap();
    writer.write_all(&[0xde, 0xad, 0xbe, 0xef]).unwrap();
    writer.finish().unwrap().into_inner()
}

/// Baseline options: whole document, LaTeX text.
fn options(format: TargetFormat, mode: Mode, export_dir: &std::path::Path) -> Options {
    Options {
        format,
        mode,
        dpi: 300.0,
        size: 12.0,
        export_dir: export_dir.to_owned(),
        scope: None,
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mt-batch-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn keep_going(_progress: &Progress) -> bool {
    true
}

#[test]
fn dry_run_statistics_count_every_formula() {
    let bytes = fixture(22);
    let doc = Document::open(&bytes).unwrap();
    let plan = plan(
        &doc,
        &options(
            TargetFormat::Latex,
            Mode::Report,
            std::path::Path::new("/tmp"),
        ),
    );
    assert_eq!(
        plan.stats,
        PlanStats {
            total: 24,
            omath: 22,
            ole: 2,
            unreadable: 2,
            planned: 24
        }
    );
    // The positions run in document order and name the carrier.
    assert_eq!(plan.items[0].location, "block 0, item 0 (Office Math)");
    assert!(
        plan.items[22].location.contains("Equation.3"),
        "{}",
        plan.items[22].location
    );
}

#[test]
fn report_mode_leaves_the_document_bytes_alone_and_lists_every_formula() {
    let bytes = fixture(22);
    let dir = temp_dir("report");
    let outcome = run(
        &bytes,
        &options(TargetFormat::Latex, Mode::Report, &dir),
        &mut keep_going,
    )
    .unwrap();
    assert!(
        outcome.docx.is_none(),
        "report mode never rewrites the container"
    );
    assert_eq!(outcome.report.total, 24);
    assert_eq!(outcome.report.succeeded, 22);
    assert_eq!(outcome.report.failures.len(), 2);
    // The 清单 holds one entry per readable formula, in order, matching the sources.
    let sources = formulas();
    for (position, entry) in outcome.report.entries.iter().take(22).enumerate() {
        let math = mt_tex::parse(sources[position]).unwrap();
        assert_eq!(entry.output, mt_tex::to_latex(&math));
    }
    // The two broken objects failed, with their 序号 and a reason each.
    for (offset, failure) in outcome.report.failures.iter().enumerate() {
        assert_eq!(failure.index, 23 + offset);
        assert!(failure.error.contains("无法读取"), "{}", failure.error);
    }
    drop(dir);
}

#[test]
fn replace_mode_converts_every_readable_formula_to_latex_text() {
    let bytes = fixture(22);
    let dir = temp_dir("replace-latex");
    let outcome = run(
        &bytes,
        &options(TargetFormat::Latex, Mode::Replace, &dir),
        &mut keep_going,
    )
    .unwrap();
    let rewritten = outcome.docx.expect("replace mode returns the container");
    assert_eq!(outcome.report.succeeded, 22);
    assert_eq!(outcome.report.failures.len(), 2);
    let doc = Document::open(&rewritten).unwrap();
    // 22 text replacements plus the two untouched broken objects.
    let equations = doc.equations();
    assert_eq!(
        equations.len(),
        2,
        "every readable formula stopped being an equation"
    );
    assert!(matches!(
        equations[0].kind,
        mt_docx::EquationKind::Ole { .. }
    ));
    // The paragraph texts now hold the LaTeX of their formulas.
    for (position, source) in formulas().into_iter().enumerate() {
        let math = mt_tex::parse(source).unwrap();
        let expected = mt_tex::to_latex(&math);
        let paragraph = &doc.blocks()[position];
        let mt_docx::Block::Paragraph(paragraph) = paragraph else {
            panic!("not a paragraph")
        };
        assert_eq!(paragraph.text(), expected, "paragraph {position}");
    }
    drop(dir);
}

#[test]
fn replace_mode_turns_latex_into_equivalent_office_math() {
    // A pure Office Math document: after replacement the count is unchanged and every tree is
    // equivalent to the one it replaced.
    let bytes = fixture(22);
    let dir = temp_dir("replace-omath");
    let outcome = run(
        &bytes,
        &options(TargetFormat::OMath, Mode::Replace, &dir),
        &mut keep_going,
    )
    .unwrap();
    assert_eq!(outcome.report.succeeded, 22);
    let doc = Document::open(&outcome.docx.unwrap()).unwrap();
    let equations = doc.equations();
    assert_eq!(
        equations.len(),
        24,
        "22 replacements plus the two broken objects"
    );
    for (position, equation) in equations.iter().take(22).enumerate() {
        let expected = mt_tex::to_latex(&mt_tex::parse(formulas()[position]).unwrap());
        let actual = mt_tex::to_latex(equation.math.as_ref().unwrap());
        assert_eq!(actual, expected, "equation {position} changed meaning");
    }
    drop(dir);
}

#[test]
fn export_mode_writes_one_png_per_readable_formula() {
    let bytes = fixture(22);
    let dir = temp_dir("export-png");
    let outcome = run(
        &bytes,
        &options(TargetFormat::Png, Mode::Export, &dir),
        &mut keep_going,
    )
    .unwrap();
    assert_eq!(outcome.report.succeeded, 22);
    let images: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .map(|e| e.path().extension().is_some_and(|e| e == "png"))
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(images.len(), 22);
    for entry in &images {
        let bytes = std::fs::read(entry.as_ref().unwrap().path()).unwrap();
        // PNG magic.
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }
    drop(dir);
}

#[test]
fn replace_mode_to_png_embeds_pictures() {
    let bytes = fixture(22);
    let dir = temp_dir("replace-png");
    let outcome = run(
        &bytes,
        &options(TargetFormat::Png, Mode::Replace, &dir),
        &mut keep_going,
    )
    .unwrap();
    assert_eq!(outcome.report.succeeded, 22);
    let doc = Document::open(&outcome.docx.unwrap()).unwrap();
    let images = doc
        .blocks()
        .iter()
        .filter_map(|block| match block {
            mt_docx::Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .flat_map(|paragraph| paragraph.items.iter())
        .filter(|item| matches!(item, mt_docx::Item::Image { .. }))
        .count();
    assert_eq!(images, 22, "every readable formula became a picture");
    drop(dir);
}

#[test]
fn a_failure_is_recorded_and_the_run_continues() {
    // Covered by every fixture test (two broken objects among the readable ones); this one pins
    // the exact bookkeeping: the failures carry their 序号 and the run is not aborted.
    let bytes = fixture(22);
    let dir = temp_dir("failures");
    let outcome = run(
        &bytes,
        &options(TargetFormat::MathMl, Mode::Report, &dir),
        &mut keep_going,
    )
    .unwrap();
    assert!(!outcome.report.aborted);
    assert_eq!(
        outcome
            .report
            .failures
            .iter()
            .map(|f| f.index)
            .collect::<Vec<_>>(),
        vec![23, 24]
    );
    let text = outcome.report.to_text();
    assert!(text.contains("#23"), "{text}");
    assert!(text.contains("无法读取"), "{text}");
    let json = outcome.report.to_json();
    assert!(json.contains("\"succeeded\":22"), "{json}");
    assert!(json.contains("\"failures\":[{\"index\":23"), "{json}");
    drop(dir);
}

#[test]
fn returning_false_from_the_callback_stops_the_run_and_keeps_the_rest() {
    let bytes = fixture(22);
    let dir = temp_dir("abort");
    let mut seen = 0;
    let outcome = run(
        &bytes,
        &options(TargetFormat::Latex, Mode::Replace, &dir),
        &mut |_progress| {
            seen += 1;
            seen < 8 // seven calls go through; the eighth stops the run
        },
    )
    .unwrap();
    assert!(outcome.report.aborted);
    // The reverse-order run handles the two broken objects first, so the seven calls that go
    // through are the two failures plus five replacements — and those five are kept.
    assert_eq!(outcome.report.succeeded, 5);
    assert_eq!(outcome.report.entries.len(), 5);
    let doc = Document::open(&outcome.docx.unwrap()).unwrap();
    // The five replacements (in reverse order: the last five readable formulas) are kept.
    let texts = doc
        .blocks()
        .iter()
        .filter_map(|block| match block {
            mt_docx::Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .filter(|paragraph| !paragraph.text().is_empty())
        .count();
    assert_eq!(texts, 5);
    drop(dir);
}

#[test]
fn scope_limits_the_run_to_formulas_inside_it() {
    let bytes = fixture(4);
    let doc = Document::open(&bytes).unwrap();
    // The byte range of the second paragraph's formula.
    let range = {
        let mt_docx::Block::Paragraph(paragraph) = &doc.blocks()[1] else {
            panic!()
        };
        paragraph.items[0].edit_range()
    };
    let dir = temp_dir("scope");
    let options = Options {
        scope: Some((range.start, range.end)),
        ..options(TargetFormat::Latex, Mode::Report, &dir)
    };
    let outcome = run(&bytes, &options, &mut keep_going).unwrap();
    assert_eq!(outcome.report.total, 1);
    assert_eq!(outcome.report.succeeded, 1);
    drop(dir);
}

#[test]
fn json_report_escapes_and_lists_everything() {
    let mut report = Report {
        format: TargetFormat::Png,
        mode: Mode::Export,
        total: 1,
        succeeded: 0,
        failures: vec![Failure {
            index: 1,
            location: "block 0, item 0 (Office Math)".to_owned(),
            error: "quote \" and newline\nend".to_owned(),
        }],
        entries: Vec::new(),
        aborted: false,
    };
    report.entries.push(Entry {
        index: 2,
        location: "block 1".to_owned(),
        output: "eq002.png".to_owned(),
    });
    let json = report.to_json();
    assert!(json.contains("\\\""), "{json}");
    assert!(json.contains("\\n"), "{json}");
    let _ = (report.format, report.mode);
}
