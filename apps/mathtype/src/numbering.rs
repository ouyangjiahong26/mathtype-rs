//! `mathtype number` and `mathtype crossref`: equation numbering and cross-references (#11).
//!
//! ```text
//! mathtype number <in.docx> [--style plain|chapter|section|paragraph] [--side right|left]
//!     [--at N] [--out <path>]
//! mathtype crossref <in.docx> [--to N|latest] [--out <path>]
//! ```
//!
//! `number` appends a Word-field number to every equation of the document — or only to equation
//! `N`, 1-based — and records the style and side in the document, so later numbering calls read
//! them back. `crossref` appends a REF field pointing at numbered equation `N` (or the most
//! recent). Both write `<名>-numbered.docx` / `<名>-crossref.docx` next to the input, or `--out`
//! when given; the input is never touched in place, because a host may have it open.

use std::path::{Path, PathBuf};

use mt_numbering::{NumberStyle, ReferenceTarget, Side};

/// Runs `number`; `Ok` carries the process exit code.
pub fn run_number(args: &[String]) -> Result<i32, String> {
    let mut input: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut style = NumberStyle::Plain;
    let mut side = Side::Right;
    let mut at: Option<usize> = None;
    let mut index = 0;
    while index < args.len() {
        let argument = args[index].as_str();
        index += 1;
        let mut value = |option: &str| -> Result<String, String> {
            args.get(index)
                .cloned()
                .ok_or_else(|| format!("{option} needs a value"))
                .inspect(|_| index += 1)
        };
        match argument {
            "--help" | "-h" => {
                println!(
                    "mathtype number <in.docx> [--style plain|chapter|section|paragraph] \
                     [--side right|left] [--at N] [--out <path>]"
                );
                return Ok(0);
            }
            "--style" => {
                style = NumberStyle::parse(&value("--style")?).ok_or_else(|| {
                    "unknown --style: plain, chapter, section, or paragraph".to_owned()
                })?;
            }
            "--side" => {
                side = Side::parse(&value("--side")?)
                    .ok_or_else(|| "unknown --side: right or left".to_owned())?;
            }
            "--at" => {
                at = Some(
                    value("--at")?
                        .parse()
                        .map_err(|_| "--at needs a 1-based equation number".to_owned())?,
                );
            }
            "--out" => out = Some(value("--out")?.into()),
            other if other.starts_with("--") => return Err(format!("unknown option: {other}")),
            other if input.is_none() => input = Some(other.into()),
            other => return Err(format!("unexpected argument: {other}")),
        }
    }
    let input = input.ok_or("usage: mathtype number <in.docx> [options]")?;
    let bytes = std::fs::read(&input)
        .map_err(|error| format!("cannot read {}: {error}", input.display()))?;
    let mut document = mt_docx::Document::open(&bytes)
        .map_err(|error| format!("cannot open {}: {error}", input.display()))?;
    let count = mt_numbering::number_existing(&mut document, style, side, at)
        .map_err(|error| format!("numbering failed: {error}"))?;
    mt_numbering::write_settings(&mut document, style, side)
        .map_err(|error| format!("saving the numbering settings failed: {error}"))?;
    let out = out.unwrap_or_else(|| default_out(&input, "numbered"));
    write(document, &out)?;
    println!(
        "{count} equations numbered ({}, {} side); written to {}",
        style.as_str(),
        side.as_str(),
        out.display()
    );
    Ok(0)
}

/// Runs `crossref`; `Ok` carries the process exit code.
pub fn run_crossref(args: &[String]) -> Result<i32, String> {
    let mut input: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut target = ReferenceTarget::Latest;
    let mut index = 0;
    while index < args.len() {
        let argument = args[index].as_str();
        index += 1;
        let mut value = |option: &str| -> Result<String, String> {
            args.get(index)
                .cloned()
                .ok_or_else(|| format!("{option} needs a value"))
                .inspect(|_| index += 1)
        };
        match argument {
            "--help" | "-h" => {
                println!("mathtype crossref <in.docx> [--to N|latest] [--out <path>]");
                return Ok(0);
            }
            "--to" => {
                let text = value("--to")?;
                target = if text.eq_ignore_ascii_case("latest") {
                    ReferenceTarget::Latest
                } else {
                    ReferenceTarget::Number(
                        text.parse()
                            .map_err(|_| "--to needs an equation number or latest".to_owned())?,
                    )
                };
            }
            "--out" => out = Some(value("--out")?.into()),
            other if other.starts_with("--") => return Err(format!("unknown option: {other}")),
            other if input.is_none() => input = Some(other.into()),
            other => return Err(format!("unexpected argument: {other}")),
        }
    }
    let input = input.ok_or("usage: mathtype crossref <in.docx> [options]")?;
    let bytes = std::fs::read(&input)
        .map_err(|error| format!("cannot read {}: {error}", input.display()))?;
    let mut document = mt_docx::Document::open(&bytes)
        .map_err(|error| format!("cannot open {}: {error}", input.display()))?;
    let bookmark =
        mt_numbering::insert_reference(&mut document, mt_docx::Position::BodyEnd, target)
            .map_err(|error| format!("inserting the cross-reference failed: {error}"))?;
    let out = out.unwrap_or_else(|| default_out(&input, "crossref"));
    write(document, &out)?;
    println!(
        "cross-reference to {bookmark} inserted; written to {}",
        out.display()
    );
    Ok(0)
}

/// `<名>-<suffix>.docx` next to the input.
fn default_out(input: &Path, suffix: &str) -> PathBuf {
    let stem = input.file_stem().map_or_else(
        || "document".to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    input.with_file_name(format!("{stem}-{suffix}.docx"))
}

fn write(document: mt_docx::Document, out: &Path) -> Result<(), String> {
    let bytes = document
        .into_bytes()
        .map_err(|error| format!("writing the document failed: {error}"))?;
    std::fs::write(out, bytes).map_err(|error| format!("cannot write {}: {error}", out.display()))
}
