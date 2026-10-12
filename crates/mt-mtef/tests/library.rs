//! Reading every equation the equation library embeds.
//!
//! The library is part of the equation editor's installation and is not in the repository. When
//! it is not on this machine the test says so and stops.

use mt_mtef::read_bare;

/// Path of the equation library.
const LIBRARY: &str = r"C:\Program Files (x86)\MathType\System\Toolbar.eql";

#[test]
fn every_equation_in_the_library_reads() {
    let Ok(bytes) = std::fs::read(LIBRARY) else {
        println!("skipping: {LIBRARY} is not on this machine");
        return;
    };

    let equations = embedded_equations(&bytes);
    println!(
        "found {} equations in {LIBRARY} ({} bytes)",
        equations.len(),
        bytes.len()
    );
    assert!(
        equations.len() >= 100,
        "the library holds a whole tool bar of equations"
    );

    let mut records = 0;
    let mut skipped = 0;
    for (offset, equation) in &equations {
        let parsed = read_bare(equation)
            .unwrap_or_else(|error| panic!("the equation at {offset:#X} does not read: {error}"));
        records += parsed.report.records;
        skipped += parsed.report.skipped;
    }
    println!("read {records} records, leaving {skipped} out of the tree");
    assert!(skipped < records, "most of every equation reaches the tree");
}

/// The equations the library holds, with the offset of each in the joined data.
///
/// The library keeps equation data in pages of 128 bytes, each introduced by a six-byte page
/// header, so the pages are joined first. An equation starts at the version byte of a stream
/// header and is read as far as it goes.
fn embedded_equations(bytes: &[u8]) -> Vec<(usize, Vec<u8>)> {
    let data = join_pages(bytes);
    let mut found = Vec::new();
    let mut offset = 0;
    while offset < data.len() {
        if looks_like_a_stream(&data[offset..]) {
            found.push((offset, data[offset..].to_vec()));
            offset += 14;
        } else {
            offset += 1;
        }
    }
    found
}

/// Join the pages of a library file.
fn join_pages(bytes: &[u8]) -> Vec<u8> {
    const PAGE: usize = 128;
    const PAGE_HEADER: usize = 6;
    const FIRST_PAGE: usize = 0x80;
    let mut data = Vec::new();
    let mut page = FIRST_PAGE;
    while page < bytes.len() {
        let start = (page + PAGE_HEADER).min(bytes.len());
        let end = (page + PAGE).min(bytes.len());
        data.extend_from_slice(&bytes[start..end]);
        page += PAGE;
    }
    data
}

/// Whether a stream header and the record after it look the way an equation begins.
fn looks_like_a_stream(data: &[u8]) -> bool {
    if data.len() < 16 || data[0] != 5 || data[1] != 1 || data[2] != 0 {
        return false;
    }
    let Some(key) = data[5..].iter().position(|&byte| byte == 0) else {
        return false;
    };
    if key == 0 || key > 32 {
        return false;
    }
    if !data[5..5 + key]
        .iter()
        .all(|&byte| (0x20..0x7F).contains(&byte))
    {
        return false;
    }
    // The application key is followed by the equation options and a definition record.
    matches!(
        data.get(7 + key),
        Some(0x08 | 0x09 | 0x0A | 0x0F | 0x10 | 0x11 | 0x12 | 0x13)
    )
}
