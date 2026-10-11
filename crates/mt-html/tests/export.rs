//! The exported page: structure, formulas, pictures, and the two output shapes.

mod util;

use mt_docx::Document;
use mt_html::{MathMode, Options, Script, export, export_files};

/// Opens the realistic fixture.
fn fixture() -> Document {
    Document::open(&util::realistic_docx()).expect("the fixture opens")
}

/// Every `img src=` of the page.
fn sources(html: &str) -> Vec<String> {
    html.split("src=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next().map(str::to_owned))
        .collect()
}

#[test]
fn headings_paragraphs_and_runs_map_to_html() {
    let html = export(&fixture(), &Options::default()).html;
    assert!(html.contains("<h1>A heading &amp; a &lt;mark&gt;</h1>"), "{html}");
    assert!(html.contains("<h2>Second</h2>"), "{html}");
    assert!(html.contains("<strong>bold text &amp; more </strong>"), "{html}");
    assert!(html.contains("<sup>2</sup>"), "{html}");
    assert!(html.contains("plain paragraph"), "{html}");
    // The opaque field degrades to its text content.
    assert!(html.contains("7"), "{html}");
    // No raw user text leaks through.
    assert!(!html.contains("<mark>"), "{html}");
}

#[test]
fn tables_become_rows_and_cells() {
    let html = export(&fixture(), &Options::default()).html;
    assert!(html.contains("<table>"), "{html}");
    assert!(html.matches("<tr>").count() == 2, "{html}");
    assert!(html.matches("<td>").count() == 4, "{html}");
    assert!(html.contains("<td><p>cell 1</p></td>"), "{html}");
    assert!(html.contains("<td><p>x=<math"), "{html}");
}

#[test]
fn formulas_become_inline_mathml() {
    let html = export(&fixture(), &Options::default()).html;
    // Inline OMath, display OMath, the object in the table, and the object after it.
    assert!(html.matches("<math").count() >= 4, "{html}");
    assert!(!html.contains("m:math"), "{html}");
    assert!(html.contains("<msqrt>"), "{html}");
    assert!(html.contains("<mfrac>"), "{html}");
}

#[test]
fn pictures_go_to_the_media_directory_once() {
    let output = export(&fixture(), &Options::default());
    assert!(output.html.contains("src=\"media/fixturepicture.png\""), "{}", output.html);
    assert!(output.html.matches("media/fixturepicture.png").count() == 1, "{}", output.html);
    assert_eq!(output.media.len(), 1, "the shared part is written once");
    assert_eq!(output.media[0].path, "media/fixturepicture.png");
    assert_eq!(output.media[0].bytes, util::PNG);
}

#[test]
fn math_image_mode_renders_pictures() {
    let options = Options { math: MathMode::Image, ..Options::default() };
    let output = export(&fixture(), &options);
    assert!(output.html.contains("img class=\"formula\""), "{}", output.html);
    assert!(output.html.matches("media/equation").count() >= 4, "{}", output.html);
    assert!(output.media.iter().all(|file| file.bytes.starts_with(&[0x89, b'P', b'N', b'G'])));
}

#[test]
fn embedded_pages_carry_their_pictures_inside() {
    let options = Options { embed: true, ..Options::default() };
    let output = export(&fixture(), &options);
    assert!(
        sources(&output.html).iter().all(|src| src.starts_with("data:image/png;base64,")),
        "{}",
        output.html
    );
    assert!(output.media.is_empty());
    assert!(!output.html.contains("src=\"media/"), "{}", output.html);
}

#[test]
fn scripts_are_injected_on_request() {
    let plain = export(&fixture(), &Options::default()).html;
    assert!(!plain.contains("<script"), "{plain}");
    let mathjax = export(&fixture(), &Options { script: Script::MathJax, ..Options::default() }).html;
    assert!(mathjax.contains("mathjax"), "{mathjax}");
    let katex = export(&fixture(), &Options { script: Script::KaTeX, ..Options::default() }).html;
    assert!(katex.contains("katex"), "{katex}");
}

#[test]
fn the_first_heading_names_the_page() {
    let html = export(&fixture(), &Options::default()).html;
    assert!(html.contains("<title>A heading &amp; a &lt;mark&gt;</title>"), "{html}");
    let titled =
        export(&fixture(), &Options { title: Some("给定标题".to_owned()), ..Options::default() }).html;
    assert!(titled.contains("<title>给定标题</title>"), "{titled}");
}

#[test]
fn the_directory_shape_writes_the_page_and_its_media() {
    let directory = std::env::temp_dir().join(format!("mt-html-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let page = export_files(&fixture(), &Options::default(), &directory).expect("the export writes");
    assert_eq!(page, directory.join("index.html"));
    let html = std::fs::read_to_string(&page).expect("the page is written");
    assert!(html.contains("src=\"media/fixturepicture.png\""));
    let picture = std::fs::read(directory.join("media/fixturepicture.png")).expect("the media is written");
    assert_eq!(picture, util::PNG);
    let _ = std::fs::remove_dir_all(&directory);
}
