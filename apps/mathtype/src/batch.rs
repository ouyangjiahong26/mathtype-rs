//! `mathtype batch-convert`: every formula of a document to another form at once (#13).
//!
//! ```text
//! mathtype batch-convert <in.docx> [--to latex|mathml|omath|png] [--mode report|replace|export]
//!     [--dpi N] [--size pt] [--out <path>] [--json] [--dry-run]
//! ```
//!
//! `--dry-run` prints the preview statistics and touches nothing. A run prints its progress
//! (`n/N`) as it goes, and Ctrl-C aborts it gracefully: everything already converted is kept and
//! the report is written all the same. The report lands next to the output (`report.txt`, plus
//! `report.json` with `--json`); the summary and every failure are printed to standard output.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use mt_batch::{self, Mode, Options, TargetFormat};

/// Whether Ctrl-C has been seen; the progress callback turns it into an abort.
static CANCELLED: AtomicBool = AtomicBool::new(false);

/// Runs the subcommand; `Ok` carries the process exit code.
pub fn run(args: &[String]) -> Result<i32, String> {
    let mut input: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut format = TargetFormat::Latex;
    let mut mode = Mode::Report;
    let mut dpi = mt_batch::DEFAULT_DPI;
    let mut size = mt_batch::DEFAULT_SIZE;
    let mut json = false;
    let mut dry_run = false;
    let mut index = 0;
    while index < args.len() {
        let argument = args[index].as_str();
        index += 1;
        let mut value = |name: &str| -> Result<String, String> {
            let value = args
                .get(index)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))?;
            index += 1;
            Ok(value)
        };
        match argument {
            "--help" | "-h" => {
                println!(
                    "mathtype batch-convert <in.docx> [--to latex|mathml|omath|png] \
                     [--mode report|replace|export] [--dpi N] [--size pt] [--out <path>] \
                     [--json] [--dry-run]\n\
                     \n\
                     --to      目标格式（默认 latex）\n\
                     --mode    report=清单不修改文档；export=逐个产物文件不修改文档；\
                     replace=文档内替换（默认 report）\n\
                     --dpi     图片分辨率（默认 300）\n\
                     --size    图片排版磅值（默认 12）\n\
                     --out     replace: 输出 docx 路径（默认 <名>-converted.docx）；\
                     report/export: 输出目录（默认 <名>-batch）\n\
                     --json    同时写出机器可读的 report.json\n\
                     --dry-run 只打印预览统计，不执行"
                );
                return Ok(0);
            }
            "--to" => {
                let name = value("--to")?;
                format = TargetFormat::parse(&name)
                    .ok_or_else(|| format!("unknown --to {name}: latex, mathml, omath, or png"))?;
            }
            "--mode" => {
                let name = value("--mode")?;
                mode = Mode::parse(&name)
                    .ok_or_else(|| format!("unknown --mode {name}: report, replace, or export"))?;
            }
            "--dpi" => {
                dpi = value("--dpi")?
                    .parse()
                    .map_err(|_| "--dpi needs a number".to_owned())?;
            }
            "--size" => {
                size = value("--size")?
                    .parse()
                    .map_err(|_| "--size needs a number".to_owned())?;
            }
            "--out" => out = Some(value("--out")?.into()),
            "--json" => json = true,
            "--dry-run" => dry_run = true,
            name if name.starts_with("--") => return Err(format!("unknown option {name}")),
            name if input.is_none() => input = Some(name.into()),
            name => return Err(format!("unexpected argument {name}")),
        }
    }
    let input = input.ok_or("usage: mathtype batch-convert <in.docx> [options]")?;
    let bytes = std::fs::read(&input)
        .map_err(|error| format!("cannot read {}: {error}", input.display()))?;
    let stem = file_stem(&input);

    if mode == Mode::Export || mode == Mode::Report {
        // Report mode lists converted text inline, but the report still lands somewhere.
        mt_batch::ensure_export_dir(&export_dir(&input, &out, &stem))?;
    }
    let options = Options {
        format,
        mode,
        dpi,
        size,
        export_dir: export_dir(&input, &out, &stem),
        scope: None,
    };
    options.validate()?;

    let document = mt_docx::Document::open(&bytes)
        .map_err(|error| format!("cannot open {}: {error}", input.display()))?;
    let plan = mt_batch::plan(&document, &options);
    if dry_run {
        println!("dry run: {}", plan.preview_text());
        for item in &plan.items {
            println!(
                "  #{} {} {}",
                item.index,
                item.location,
                if item.readable { "" } else { "[无法解析]" }
            );
        }
        return Ok(0);
    }

    // Ctrl-C flips the flag; the progress callback answers `false` and the run stops, keeping
    // everything already converted.
    let _ = ctrlc::set_handler(|| CANCELLED.store(true, Ordering::SeqCst));
    let mut seen_cancel = false;
    let outcome = mt_batch::run(&bytes, &options, &mut |progress| {
        let cancelled = CANCELLED.load(Ordering::SeqCst);
        if cancelled {
            if !seen_cancel {
                eprintln!("interrupted; stopping after this item's predecessors — 已完成项保留");
                seen_cancel = true;
            }
            return false;
        }
        println!(
            "{}/{} {}",
            progress.index, progress.total, progress.location
        );
        true
    })?;
    let report = &outcome.report;

    let report_dir = match mode {
        Mode::Replace => {
            let target =
                out.unwrap_or_else(|| input.with_file_name(format!("{stem}-converted.docx")));
            if let Some(docx) = &outcome.docx {
                std::fs::write(&target, docx)
                    .map_err(|error| format!("cannot write {}: {error}", target.display()))?;
                println!("converted document: {}", target.display());
            }
            target
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        }
        _ => export_dir(&input, &out, &stem),
    };
    let text_path = report_dir.join("report.txt");
    std::fs::write(&text_path, report.to_text())
        .map_err(|error| format!("cannot write {}: {error}", text_path.display()))?;
    let json_path = report_dir.join("report.json");
    if json {
        std::fs::write(&json_path, report.to_json())
            .map_err(|error| format!("cannot write {}: {error}", json_path.display()))?;
        println!("report: {} {}", text_path.display(), json_path.display());
    } else {
        println!("report: {}", text_path.display());
    }

    println!(
        "批量转换{}：共 {} 个，成功 {}，失败 {}{}",
        if report.aborted {
            "已中止"
        } else {
            "完成"
        },
        report.total,
        report.succeeded,
        report.failures.len(),
        if report.failures.is_empty() {
            String::new()
        } else {
            "；失败项:".to_owned()
        }
    );
    for failure in &report.failures {
        println!(
            "  #{} {}: {}",
            failure.index, failure.location, failure.error
        );
    }
    Ok(if report.aborted || !report.failures.is_empty() {
        1
    } else {
        0
    })
}

/// `<名>-batch` next to the input, or `--out` when given.
fn export_dir(input: &Path, out: &Option<PathBuf>, stem: &str) -> PathBuf {
    out.clone()
        .unwrap_or_else(|| input.with_file_name(format!("{stem}-batch")))
}

/// The file name of `path` without its last extension.
fn file_stem(path: &Path) -> String {
    path.file_stem().map_or_else(
        || "document".to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    )
}
