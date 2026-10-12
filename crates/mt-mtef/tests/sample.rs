//! Reading the equation object a blank document ships with.
//!
//! The object is not part of the repository: it is the equation a document the equation editor
//! installs carries. When it is not on this machine the test says so and stops.

use mt_core::{MathStyle, Node};
use mt_mtef::{Stream, parse, read, write};

/// The path the sample is read from: `MTEF_SAMPLE` when set, nothing otherwise.
fn sample_path() -> Option<String> {
    std::env::var("MTEF_SAMPLE").ok()
}

#[test]
fn reads_a_blank_equation_object() {
    let Some(path) = sample_path() else {
        println!("skipping: set MTEF_SAMPLE to the path of the extracted equation object");
        return;
    };
    let Ok(bytes) = std::fs::read(&path) else {
        println!("skipping: {path} is not on this machine (set MTEF_SAMPLE to override)");
        return;
    };
    assert_eq!(bytes.len(), 220, "the object is the blank equation");

    let parsed = read(&bytes).expect("the equation object reads");

    // What the stream says about itself.
    assert_eq!(parsed.header.version, 5);
    assert_eq!(parsed.header.platform, 1);
    assert_eq!(parsed.header.product, 0);
    assert_eq!(parsed.header.creator, "DSMT6");
    assert_eq!(parsed.header.options, 0);
    assert_eq!(
        parsed.header.encodings,
        vec!["WinAllBasicCodePages".to_owned()]
    );
    let names: Vec<&str> = parsed
        .header
        .fonts
        .iter()
        .map(|font| font.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["Times New Roman", "Symbol", "Courier New", "MT Extra"]
    );
    assert_eq!(
        parsed
            .header
            .fonts
            .iter()
            .map(|font| font.encoding)
            .collect::<Vec<_>>(),
        vec![5, 3, 5, 4],
        "the fonts name the encodings the stream defines"
    );

    // The preferences the document was written with.
    let preferences = &parsed.header.preferences;
    assert_eq!(preferences.sizes.len(), 8);
    assert_eq!(preferences.spaces.len(), 30);
    assert_eq!(preferences.styles.len(), 12);
    assert_eq!(
        preferences.sizes[0].value, "12",
        "the full size is twelve points"
    );
    assert_eq!(
        preferences.styles[2].style, 2,
        "the variable style is italic"
    );

    // A blank equation carries no content, only its size.
    assert_eq!(parsed.math.body, Node::Row(Vec::new()));
    assert_eq!(parsed.math.style, MathStyle::Full);
    assert_eq!(
        parsed.report.records, 8,
        "the object holds only definition records"
    );
    assert_eq!(parsed.report.skipped, 0);

    // The tree survives a write followed by a read.
    let stream = write(&parsed.math);
    let again = parse(&stream).expect("a written object reads");
    assert_eq!(again, parsed.math);

    // So does the object itself, taken apart and put back together.
    let split = Stream::split(&bytes).expect("the object splits");
    assert_eq!(split.preamble.len(), 28);
    assert_eq!(split.body.len(), 192);
    assert_eq!(split.into_bytes(), bytes);

    // Every cut of the object reads or fails, and none of them panics.
    for length in 0..bytes.len() {
        let _ = mt_mtef::read(&bytes[..length]);
    }
}
