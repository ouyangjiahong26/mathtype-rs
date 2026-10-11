//! Document-level batch conversion: every formula of a Word document to another form at once.
//!
//! The pipeline serves #13 (文档级批量转换) and has two halves:
//!
//! * [`plan`] reads a [`mt_docx::Document`], locates every formula with [`Document::equations`],
//!   and produces the per-item plan (position, carrier kind, target action) plus the preview
//!   statistics a `--dry-run` prints: how many formulas there are, split by carrier, and how many
//!   would be acted on.
//! * [`run`] executes the plan item by item. Replacement mode rewrites the document **in reverse
//!   document order**, so the coordinates of the items still pending are never invalidated by an
//!   edit (the same trick the `mt-docx` edit tests use); a failure is recorded (序号, 位置, 原因)
//!   and the run continues; the progress callback returning `false` aborts, keeping everything
//!   already done.
//!
//! # Modes
//!
//! * [`Mode::Report`] — the document is not modified at all; the report lists every formula with
//!   its converted text (LaTeX/MathML/OMML) inline, the 清单 of a dry inspection.
//! * [`Mode::Export`] — the document is not modified; one product file per formula
//!   (`eq001.tex` / `.mml` / `.omml` / `.png`) is written into the export directory.
//! * [`Mode::Replace`] — the document is rewritten: every formula item becomes the target form
//!   (LaTeX/MathML text run, Office Math, or an embedded picture).
//!
//! # Scope
//!
//! CLI and bridge callers convert whole documents. A host that knows a selection (the Word
//! add-in) can pass [`Options::scope`] — a byte range of `word/document.xml` — and only formulas
//! whose markup lies inside it are converted.

use std::fmt::Write as _;
use std::path::Path;

use mt_core::Math;
use mt_docx::{Document, Equation, EquationKind, Replacement, RunStyle};
use mt_layout::{LayoutStyle, layout};
use mt_render::RenderOptions;
use mt_render::metrics::{FaceMetrics, MATH_FONT};

/// The form every formula is converted into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetFormat {
    /// LaTeX source text.
    Latex,
    /// MathML markup.
    MathMl,
    /// Office Math (the native Word equation), as markup or as a replacement item.
    OMath,
    /// A rendered PNG picture.
    Png,
}

impl TargetFormat {
    /// The name the CLI and the bridge accept.
    pub const NAMES: [&'static str; 4] = ["latex", "mathml", "omath", "png"];

    /// Parses a `--to` / `?to=` value.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "latex" => Some(Self::Latex),
            "mathml" => Some(Self::MathMl),
            "omath" => Some(Self::OMath),
            "png" => Some(Self::Png),
            _ => None,
        }
    }

    /// The name, for reports.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Latex => "latex",
            Self::MathMl => "mathml",
            Self::OMath => "omath",
            Self::Png => "png",
        }
    }

    /// The human label, for reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Latex => "LaTeX 文本",
            Self::MathMl => "MathML 文本",
            Self::OMath => "Office 原生公式",
            Self::Png => "PNG 图片",
        }
    }

    /// The extension of an exported product file.
    const fn extension(self) -> &'static str {
        match self {
            Self::Latex => "tex",
            Self::MathMl => "mml",
            Self::OMath => "omml",
            Self::Png => "png",
        }
    }
}

/// What happens to the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Convert nothing in the document; list every formula with its converted text.
    Report,
    /// Convert nothing in the document; write one product file per formula.
    Export,
    /// Rewrite the document, replacing every formula with its target form.
    Replace,
}

impl Mode {
    /// Parses a `--mode` / `?mode=` value.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "report" => Some(Self::Report),
            "export" => Some(Self::Export),
            "replace" => Some(Self::Replace),
            _ => None,
        }
    }

    /// The name, for reports.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Report => "report",
            Self::Export => "export",
            Self::Replace => "replace",
        }
    }

    /// The human label, for reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Report => "清单（不修改文档）",
            Self::Export => "导出（不修改文档）",
            Self::Replace => "替换（重写文档）",
        }
    }
}

/// Smallest and largest accepted image resolution, in dots per inch.
pub const MIN_DPI: f64 = 36.0;
/// Default image resolution, in dots per inch.
pub const DEFAULT_DPI: f64 = 300.0;
/// Smallest and largest layout size, in points.
pub const MIN_SIZE: f64 = 4.0;
/// Default layout size for pictures, in points (matches Word's inline 12 pt equation).
pub const DEFAULT_SIZE: f64 = 12.0;

/// What a run converts and where its products go.
#[derive(Clone, Debug)]
pub struct Options {
    /// The form every formula becomes.
    pub format: TargetFormat,
    /// Whether the document is rewritten, products are exported, or only listed.
    pub mode: Mode,
    /// Image resolution in dots per inch; the layout scale is `dpi / 72`.
    pub dpi: f64,
    /// Layout size in points, the size a replaced picture claims in the document.
    pub size: f64,
    /// Where `Mode::Export` writes its product files (and callers may drop reports).
    pub export_dir: std::path::PathBuf,
    /// A byte range of `word/document.xml`: only formulas whose markup lies inside it are
    /// converted. `None` converts the whole document.
    pub scope: Option<(usize, usize)>,
}

impl Options {
    /// Checks the numeric bounds the callers cannot be trusted with.
    pub fn validate(&self) -> Result<(), String> {
        if !(MIN_DPI..=1200.0).contains(&self.dpi) || !self.dpi.is_finite() {
            return Err(format!("dpi must lie between {MIN_DPI} and 1200"));
        }
        if !(MIN_SIZE..=200.0).contains(&self.size) || !self.size.is_finite() {
            return Err(format!("size must lie between {MIN_SIZE} and 200 points"));
        }
        Ok(())
    }
}

/// One formula the plan will act on.
#[derive(Clone, Debug)]
pub struct PlanItem {
    /// The 1-based position in document order, the 序号 every failure is reported under.
    pub index: usize,
    /// The block the formula sits in.
    pub block: usize,
    /// The item inside that block.
    pub item: usize,
    /// The carrier: `omath` or `ole`.
    pub kind: &'static str,
    /// Where the formula is, for humans: `block 3, item 0 (Office Math)`.
    pub location: String,
    /// The display form, when the carrier is Office Math.
    pub display: bool,
    /// Whether the carrier's bytes could be read into a tree; unreadable items will fail.
    pub readable: bool,
}

/// The preview statistics of a plan.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlanStats {
    /// Formulas in scope, whatever carries them.
    pub total: usize,
    /// Of them, the Office Math ones.
    pub omath: usize,
    /// Of them, the embedded OLE objects.
    pub ole: usize,
    /// Of them, the ones whose tree could not be read; they are expected to fail.
    pub unreadable: usize,
    /// How many items the mode would act on: everything in scope, for every mode counts.
    pub planned: usize,
}

/// The enumerated formulas and their statistics.
#[derive(Clone, Debug)]
pub struct Plan {
    /// The items, in document order.
    pub items: Vec<PlanItem>,
    /// The counts a preview prints.
    pub stats: PlanStats,
}

impl Plan {
    /// The human preview a `--dry-run` prints.
    pub fn preview_text(&self) -> String {
        let stats = &self.stats;
        let mut out = String::new();
        let _ = writeln!(
            out,
            "找到公式 {} 个：Office Math {} 个，嵌入式对象 {} 个",
            stats.total, stats.omath, stats.ole
        );
        if stats.unreadable > 0 {
            let _ = writeln!(
                out,
                "其中 {} 个无法解析为公式树（转换时将计为失败）",
                stats.unreadable
            );
        }
        let _ = write!(out, "预计处理 {} 个", stats.planned);
        out
    }
}

/// Locates every formula in scope, with the plan facts of each.
fn enumerate(doc: &Document, options: &Options) -> (Vec<(Equation, PlanItem)>, PlanStats) {
    let mut pairs = Vec::new();
    for equation in doc.equations() {
        let Some(entry) = doc.blocks().get(equation.block) else {
            continue;
        };
        let mt_docx::Block::Paragraph(paragraph) = entry else {
            continue;
        };
        let range = paragraph
            .items
            .get(equation.item)
            .map(|item| item.edit_range())
            .unwrap_or_default();
        if let Some((start, end)) = options.scope {
            if range.start < start || range.end > end {
                continue;
            }
        }
        let (kind, label, display) = match &equation.kind {
            EquationKind::Omath { display } => ("omath", "Office Math", *display),
            EquationKind::Ole { prog_id, .. } => {
                ("ole", prog_id.as_deref().unwrap_or("嵌入式对象"), false)
            }
        };
        let readable = equation.math.is_some();
        let location = format!(
            "block {}, item {} ({})",
            equation.block, equation.item, label
        );
        let index = pairs.len() + 1;
        let (block, item) = (equation.block, equation.item);
        let plan_item = PlanItem {
            index,
            block,
            item,
            kind,
            location,
            display,
            readable,
        };
        pairs.push((equation, plan_item));
    }
    let stats = PlanStats {
        total: pairs.len(),
        omath: pairs
            .iter()
            .filter(|(equation, _)| matches!(equation.kind, EquationKind::Omath { .. }))
            .count(),
        ole: pairs
            .iter()
            .filter(|(equation, _)| matches!(equation.kind, EquationKind::Ole { .. }))
            .count(),
        unreadable: pairs.iter().filter(|(_, item)| !item.readable).count(),
        planned: pairs.len(),
    };
    (pairs, stats)
}

/// Produces the plan and its preview statistics; nothing is executed.
pub fn plan(doc: &Document, options: &Options) -> Plan {
    let (pairs, stats) = enumerate(doc, options);
    Plan {
        items: pairs.into_iter().map(|(_, item)| item).collect(),
        stats,
    }
}

/// Where a run is, announced before each item.
#[derive(Clone, Debug)]
pub struct Progress {
    /// The 1-based position of the item about to be processed.
    pub index: usize,
    /// How many items the run covers.
    pub total: usize,
    /// Where the item is, for humans.
    pub location: String,
}

/// One item that failed: its 序号, its position, and why.
#[derive(Clone, Debug)]
pub struct Failure {
    /// The 1-based position in document order.
    pub index: usize,
    /// Where the formula was.
    pub location: String,
    /// The human-readable reason.
    pub error: String,
}

/// One item that was converted: its 序号, its position, and what came out.
#[derive(Clone, Debug)]
pub struct Entry {
    /// The 1-based position in document order.
    pub index: usize,
    /// Where the formula was.
    pub location: String,
    /// The converted text (report mode) or the product file name (export mode); for replacements,
    /// a short note of what replaced the formula.
    pub output: String,
}

/// The summary of a finished (or aborted) run.
#[derive(Clone, Debug)]
pub struct Report {
    /// The form that was requested.
    pub format: TargetFormat,
    /// The mode that ran.
    pub mode: Mode,
    /// Items the run covered.
    pub total: usize,
    /// Items converted without error.
    pub succeeded: usize,
    /// Items that failed, in document order.
    pub failures: Vec<Failure>,
    /// Items converted, in document order.
    pub entries: Vec<Entry>,
    /// Whether the run stopped early; everything done before then is kept.
    pub aborted: bool,
}

impl Report {
    /// The human-readable report file.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "批量转换报告");
        let _ = writeln!(
            out,
            "目标格式: {}；模式: {}",
            self.format.label(),
            self.mode.label()
        );
        let _ = writeln!(
            out,
            "公式总数: {}；成功: {}；失败: {}；中止: {}",
            self.total,
            self.succeeded,
            self.failures.len(),
            if self.aborted { "是" } else { "否" }
        );
        if !self.failures.is_empty() {
            let _ = writeln!(out, "失败项:");
            for failure in &self.failures {
                let _ = writeln!(
                    out,
                    "  #{} {}: {}",
                    failure.index, failure.location, failure.error
                );
            }
        }
        if !self.entries.is_empty() {
            let _ = writeln!(out, "逐项结果:");
            for entry in &self.entries {
                let _ = writeln!(
                    out,
                    "  #{} {}: {}",
                    entry.index, entry.location, entry.output
                );
            }
        }
        out
    }

    /// The machine-readable report (`--json`).
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        let _ = write!(
            out,
            "{{\"format\":\"{}\",\"mode\":\"{}\",\"total\":{},\"succeeded\":{},\"aborted\":{}",
            self.format.name(),
            self.mode.name(),
            self.total,
            self.succeeded,
            self.aborted
        );
        out.push_str(",\"failures\":[");
        for (position, failure) in self.failures.iter().enumerate() {
            if position > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"index\":{},\"location\":\"{}\",\"error\":\"{}\"}}",
                failure.index,
                json_escape(&failure.location),
                json_escape(&failure.error)
            );
        }
        out.push_str("],\"entries\":[");
        for (position, entry) in self.entries.iter().enumerate() {
            if position > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"index\":{},\"location\":\"{}\",\"output\":\"{}\"}}",
                entry.index,
                json_escape(&entry.location),
                json_escape(&entry.output)
            );
        }
        out.push_str("]}");
        out
    }
}

/// What a run produced.
pub struct Outcome {
    /// The per-item summary.
    pub report: Report,
    /// The rewritten container, in replace mode only.
    pub docx: Option<Vec<u8>>,
}

/// Largest PNG a batch will allocate, matching the single-conversion CLI's guard.
const MAX_RENDER_PIXELS: u64 = 1 << 27;

/// Executes the plan item by item.
///
/// The `progress` callback is invoked before each item; returning `false` aborts the run, keeping
/// every item already processed (and, in replace mode, the edits already made — the returned
/// container still contains them).
pub fn run(
    bytes: &[u8],
    options: &Options,
    progress: &mut dyn FnMut(&Progress) -> bool,
) -> Result<Outcome, String> {
    options.validate()?;
    let mut doc =
        Document::open(bytes).map_err(|error| format!("cannot open the document: {error}"))?;
    let (mut pairs, stats) = enumerate(&doc, options);
    let mut report = Report {
        format: options.format,
        mode: options.mode,
        total: stats.total,
        succeeded: 0,
        failures: Vec::new(),
        entries: Vec::new(),
        aborted: false,
    };
    if report.total == 0 {
        return Ok(Outcome { report, docx: None });
    }
    if options.mode == Mode::Replace {
        // Reverse document order: an edit never shifts the coordinates of the items still pending.
        pairs.reverse();
    }
    for (equation, item) in pairs {
        let go_on = progress(&Progress {
            index: item.index,
            total: report.total,
            location: item.location.clone(),
        });
        if !go_on {
            report.aborted = true;
            break;
        }
        match convert_item(&mut doc, &equation, &item, options) {
            Ok(output) => {
                report.succeeded += 1;
                report.entries.push(Entry {
                    index: item.index,
                    location: item.location.clone(),
                    output,
                });
            }
            Err(error) => {
                report.failures.push(Failure {
                    index: item.index,
                    location: item.location.clone(),
                    error,
                });
            }
        }
    }
    // Reports read in document order whatever order the work ran in.
    report.entries.sort_by_key(|entry| entry.index);
    report.failures.sort_by_key(|failure| failure.index);
    let docx = if options.mode == Mode::Replace {
        Some(
            doc.into_bytes()
                .map_err(|error| format!("cannot write the converted document: {error}"))?,
        )
    } else {
        None
    };
    Ok(Outcome { report, docx })
}

/// Converts one located formula; `Ok` carries the note the report lists for it.
fn convert_item(
    doc: &mut Document,
    equation: &Equation,
    item: &PlanItem,
    options: &Options,
) -> Result<String, String> {
    let math = equation
        .math
        .clone()
        .ok_or_else(|| "无法读取公式树（Office Math 解析失败或对象流损坏）".to_string())?;
    match options.format {
        TargetFormat::Latex => {
            let latex = mt_tex::to_latex(&math);
            match options.mode {
                Mode::Report => Ok(latex),
                Mode::Export => export_text(doc, item, &latex, options),
                Mode::Replace => replace_text(doc, equation, item, &latex),
            }
        }
        TargetFormat::MathMl => {
            let markup = mt_mathml::write(&math);
            match options.mode {
                Mode::Report => Ok(markup),
                Mode::Export => export_text(doc, item, &markup, options),
                Mode::Replace => replace_text(doc, equation, item, &markup),
            }
        }
        TargetFormat::OMath => {
            let markup = mt_omml::write(&math);
            match options.mode {
                Mode::Report => Ok(markup),
                Mode::Export => export_text(doc, item, &markup, options),
                Mode::Replace => {
                    doc.replace(
                        equation.block,
                        equation.item,
                        Replacement::OMath {
                            math,
                            display: item.display,
                        },
                    )
                    .map_err(|error| format!("替换失败: {error}"))?;
                    Ok("已替换为 Office 原生公式".to_string())
                }
            }
        }
        TargetFormat::Png => {
            let (bytes, width, height) = render_png(&math, options)?;
            match options.mode {
                Mode::Report => Ok(format!(
                    "{width}×{height} px（清单模式不写出文件；用 --mode export 导出图片）"
                )),
                Mode::Export => {
                    let name = product_name(item, options.format.extension());
                    let path = options.export_dir.join(&name);
                    std::fs::write(&path, &bytes)
                        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
                    Ok(name)
                }
                Mode::Replace => {
                    let emu = pixels_to_emu(width, height, options.dpi);
                    let data = mt_docx::ImageData {
                        bytes,
                        extension: "png".to_owned(),
                        content_type: "image/png".to_owned(),
                        name: format!("equation-{}", item.index),
                        extent_emu: emu,
                    };
                    doc.replace(equation.block, equation.item, Replacement::Image(data))
                        .map_err(|error| format!("替换失败: {error}"))?;
                    Ok(format!("已替换为图片 {width}×{height} px").to_string())
                }
            }
        }
    }
}

/// Replaces one formula with a text run holding `text`.
fn replace_text(
    doc: &mut Document,
    equation: &Equation,
    _item: &PlanItem,
    text: &str,
) -> Result<String, String> {
    doc.replace(
        equation.block,
        equation.item,
        Replacement::Text {
            text: text.to_owned(),
            style: RunStyle::default(),
        },
    )
    .map_err(|error| format!("替换失败: {error}"))?;
    Ok(format!("已替换为文本: {text}"))
}

/// Writes one text product file; `Ok` carries its name.
fn export_text(
    _doc: &Document,
    item: &PlanItem,
    text: &str,
    options: &Options,
) -> Result<String, String> {
    let name = product_name(item, options.format.extension());
    let path = options.export_dir.join(&name);
    std::fs::write(&path, text)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(name)
}

/// `eq001.tex`, zero-padded so the files sort in document order.
fn product_name(item: &PlanItem, extension: &str) -> String {
    format!("eq{:03}.{extension}", item.index)
}

/// Renders one tree to PNG bytes and its pixel size.
fn render_png(math: &Math, options: &Options) -> Result<(Vec<u8>, u32, u32), String> {
    let metrics =
        FaceMetrics::parse(MATH_FONT, 0).ok_or("bundled math font could not be parsed")?;
    let style = LayoutStyle::display(options.size);
    let boxes = layout(math, &metrics, &style);
    let render = RenderOptions {
        scale: options.dpi / 72.0,
        ..RenderOptions::default()
    };
    let (width, height) = mt_render::image_size(&boxes, &style, metrics.face(), &render);
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_RENDER_PIXELS {
        return Err(format!(
            "图片会渲染为 {width}×{height} 像素（{pixels} 总量），超过 {MAX_RENDER_PIXELS} 像素上限；降低 --dpi 或缩短公式"
        ));
    }
    let bytes = mt_render::render_png(&boxes, &style, metrics.face(), &render)
        .map_err(|error| format!("cannot rasterise: {error}"))?;
    Ok((bytes, width, height))
}

/// Pixel size to English Metric Units at the run's resolution.
fn pixels_to_emu(width: u32, height: u32, dpi: f64) -> (u32, u32) {
    let factor = 914400.0 / dpi;
    (
        (f64::from(width) * factor).round() as u32,
        (f64::from(height) * factor).round() as u32,
    )
}

/// Escapes a string for the hand-written JSON report.
fn json_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if (character as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", character as u32);
            }
            character => out.push(character),
        }
    }
    out
}

/// Creates the directory products are written into.
pub fn ensure_export_dir(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))
}

#[cfg(test)]
mod tests;
