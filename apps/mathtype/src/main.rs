//! Equation editor: palette toolbar, canvas editing, format conversion.
//!
//! Three ways in:
//! - no arguments: the graphical editor (palettes, canvas, caret, LaTeX/MathML/OMML import);
//! - `--to <format> [--from <format>] [--in file] [--out file] <text>`: one conversion, for
//!   scripts and for host add-ins that can launch a process;
//! - `--serve [port]`: the same conversions over loopback HTTP, for host add-ins that cannot
//!   (WPS loads its add-ins as web pages).

mod library;
mod serve;

use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Pos2, Rect, Sense,
    Stroke, Vec2, ViewportCommand,
};
use mt_core::{Font, MathStyle, Side};
use mt_edit::{Cursor, Editor, PaletteItem, palettes};
use mt_layout::{layout, layout_with_anchors, to_svg, Box as LayoutBox, LayoutStyle, RowAnchor};
use mt_render::metrics::{FaceMetrics, MATH_FONT};
use mt_style_preset::StylePreset;

const MATH_FAMILY: &str = "math";

/// System faces that can supply the CJK glyphs of the interface labels, best first.
const UI_FONT_CANDIDATES: [&str; 4] = [
    r"C:\Windows\Fonts\simhei.ttf",
    r"C:\Windows\Fonts\msyh.ttc",
    r"C:\Windows\Fonts\simsun.ttc",
    r"C:\Windows\Fonts\Deng.ttf",
];

/// Input and output formats the command line converts between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Latex,
    MathMl,
    Omml,
    Mtef,
    Svg,
    Png,
    Eps,
}

impl Format {
    fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "latex" | "tex" => Some(Format::Latex),
            "mathml" | "mml" => Some(Format::MathMl),
            "omml" | "omath" => Some(Format::Omml),
            "mtef" | "eqn" => Some(Format::Mtef),
            "svg" => Some(Format::Svg),
            "png" => Some(Format::Png),
            "eps" => Some(Format::Eps),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Format::Latex => "latex",
            Format::MathMl => "mathml",
            Format::Omml => "omml",
            Format::Mtef => "mtef",
            Format::Svg => "svg",
            Format::Png => "png",
            Format::Eps => "eps",
        }
    }

    /// Whether the format's bytes are not text, so a text clipboard cannot carry it.
    fn is_binary(self) -> bool {
        matches!(self, Format::Mtef | Format::Png)
    }
}

/// What the command line was asked to do.
struct Job {
    from: Option<Format>,
    to: Format,
    input: Option<String>,
    input_file: Option<std::path::PathBuf>,
    out: Option<std::path::PathBuf>,
    size: f64,
    /// Device pixels per point when rasterising.
    scale: f64,
}

/// Smallest and largest layout size, in points, accepted by the command line and the HTTP API.
pub(crate) const MIN_SIZE: f64 = 4.0;
pub(crate) const MAX_SIZE: f64 = 200.0;

/// Smallest and largest device-pixel scale accepted by the command line and the HTTP API.
pub(crate) const MIN_SCALE: f64 = 1.0;
pub(crate) const MAX_SCALE: f64 = 16.0;

/// Reject sizes that are not finite or outside [`MIN_SIZE`]..=[`MAX_SIZE`].
///
/// A layout size is turned into a device buffer directly, so an unbounded value such as
/// `9999999` would ask for an impossible allocation and abort the process.
pub(crate) fn validate_size(size: f64) -> Result<f64, String> {
    if size.is_finite() && (MIN_SIZE..=MAX_SIZE).contains(&size) {
        Ok(size)
    } else {
        Err(format!("size must be between {MIN_SIZE} and {MAX_SIZE} points"))
    }
}

/// Reject scales that are not finite or outside [`MIN_SCALE`]..=[`MAX_SCALE`].
pub(crate) fn validate_scale(scale: f64) -> Result<f64, String> {
    if scale.is_finite() && (MIN_SCALE..=MAX_SCALE).contains(&scale) {
        Ok(scale)
    } else {
        Err(format!("scale must be between {MIN_SCALE} and {MAX_SCALE}"))
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--serve") {
        let port = args.get(1).and_then(|value| value.parse().ok()).unwrap_or(8760);
        if let Err(error) = serve::serve(port) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("export-html") {
        return match export_html(&args[1..]) {
            Ok(()) => Ok(()),
            Err(message) => {
                eprintln!("{message}");
                std::process::exit(1);
            }
        };
    }
    if args.first().map(String::as_str) == Some("--edit") {
        let path = args.get(1).cloned().unwrap_or_else(|| "equation.tex".to_owned());
        return run_editor(std::path::PathBuf::from(path));
    }
    if let Some(code) = run_cli(&args) {
        std::process::exit(code);
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native("mathtype-rs", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

/// `mathtype export-html <in.docx> <outdir> [--script=...] [--math=...]`: the document to a
/// web page directory, formulas as MathML unless pictures are asked for.
fn export_html(args: &[String]) -> Result<(), String> {
    let mut input: Option<std::path::PathBuf> = None;
    let mut outdir: Option<std::path::PathBuf> = None;
    let mut options = mt_html::Options::default();
    let mut index = 0;
    while index < args.len() {
        let argument = args[index].as_str();
        index += 1;
        match argument {
            "--help" | "-h" => {
                println!("mathtype export-html <in.docx> <outdir> [--script=none|mathjax|katex] [--math=mathml|image]");
                return Ok(());
            }
            "--script" => {
                let value = read_value(args, &mut index, "--script")?;
                options.script = mt_html::Script::parse(&value)
                    .ok_or_else(|| format!("unknown --script {value}: none, mathjax, or katex"))?;
            }
            "--math" => {
                let value = read_value(args, &mut index, "--math")?;
                options.math = mt_html::MathMode::parse(&value)
                    .ok_or_else(|| format!("unknown --math {value}: mathml or image"))?;
            }
            name if let Some(value) = name.strip_prefix("--script=") => {
                options.script = mt_html::Script::parse(value)
                    .ok_or_else(|| format!("unknown --script {value}: none, mathjax, or katex"))?;
            }
            name if let Some(value) = name.strip_prefix("--math=") => {
                options.math = mt_html::MathMode::parse(value)
                    .ok_or_else(|| format!("unknown --math {value}: mathml or image"))?;
            }
            name if name.starts_with("--") => return Err(format!("unknown option {name}")),
            name if input.is_none() => input = Some(name.into()),
            name if outdir.is_none() => outdir = Some(name.into()),
            name => return Err(format!("unexpected argument {name}")),
        }
    }
    let input = input.ok_or("usage: mathtype export-html <in.docx> <outdir>")?;
    let outdir = outdir.ok_or("usage: mathtype export-html <in.docx> <outdir>")?;
    let bytes = std::fs::read(&input)
        .map_err(|error| format!("cannot read {}: {error}", input.display()))?;
    let document = mt_docx::Document::open(&bytes)
        .map_err(|error| format!("cannot open {}: {error}", input.display()))?;
    let page = mt_html::export_files(&document, &options, &outdir)?;
    println!("{} written", page.display());
    Ok(())
}

/// Open the editor on an equation stored as text in `path`, writing it back when the user saves.
///
/// This is the hand-off used when an embedded object is opened from a document: the object server
/// writes the equation out, starts this process, and picks the result up afterwards.
fn run_editor(path: std::path::PathBuf) -> eframe::Result<()> {
    let title = format!("公式编辑器 — {}", path.file_name().map_or_else(|| "equation".into(), |name| name.to_string_lossy().into_owned()));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 560.0]).with_title(title),
        ..Default::default()
    };
    eframe::run_native(
        "mathtype-rs",
        options,
        Box::new(move |cc| Ok(Box::new(App::editing(cc, path)))),
    )
}

/// Run a headless conversion when the command line asked for one. `None` starts the editor.
fn run_cli(args: &[String]) -> Option<i32> {
    if args.is_empty() {
        return None;
    }
    let job = match parse_job(args) {
        Ok(Some(job)) => job,
        Ok(None) => return Some(0),
        Err(message) => {
            eprintln!("{message}");
            return Some(2);
        }
    };
    match execute(job) {
        Ok(()) => Some(0),
        Err(message) => {
            eprintln!("{message}");
            Some(1)
        }
    }
}

fn usage() {
    println!(
        "mathtype [options] [<text>]\n\
         \n\
         Formats: latex, mathml, omml, svg, png, eps\n\
         \n\
         --from <format>  input format; inferred from the text when omitted\n\
         --to <format>    output format (default: svg)\n\
         --in <file>      read the input from a file instead of the argument\n\
         --out <file>     write the result to a file instead of standard output\n\
         --size <points>  full size used when laying the equation out (default: 18)\n\
         --scale <factor> device pixels per point for png (default: 3)\n\
         \n\
         Examples:\n\
         mathtype '\\frac{{a}}{{b}}' --to omml\n\
         mathtype --from omml --to latex --in equation.xml\n\
         mathtype export-html doc.docx web/   # --script=mathjax|katex|none, --math=mathml|image\n\
         mathtype export-html doc.docx web/  # --script=mathjax|katex|none, --math=mathml|image\n\
         mathtype --serve 8760   # POST /convert?from=latex&to=omml for host add-ins"
    );
}

fn parse_job(args: &[String]) -> Result<Option<Job>, String> {
    let mut job = Job {
        from: None,
        to: Format::Svg,
        input: None,
        input_file: None,
        out: None,
        size: 18.0,
        scale: 3.0,
    };
    let mut index = 0;
    while index < args.len() {
        let argument = args[index].as_str();
        index += 1;
        match argument {
            "--help" | "-h" => {
                usage();
                return Ok(None);
            }
            "--from" => job.from = Some(read_format(args, &mut index, "--from")?),
            "--to" => job.to = read_format(args, &mut index, "--to")?,
            "--in" => job.input_file = Some(read_value(args, &mut index, "--in")?.into()),
            "--out" => job.out = Some(read_value(args, &mut index, "--out")?.into()),
            "--size" => {
                let value: f64 = read_value(args, &mut index, "--size")?
                    .parse()
                    .map_err(|_| "--size needs a number of points".to_owned())?;
                job.size = validate_size(value).map_err(|message| format!("--size {message}"))?;
            }
            "--scale" => {
                let value: f64 = read_value(args, &mut index, "--scale")?
                    .parse()
                    .map_err(|_| "--scale needs a positive number".to_owned())?;
                job.scale = validate_scale(value).map_err(|message| format!("--scale {message}"))?;
            }
            other if other.starts_with("--") => return Err(format!("unknown option: {other}")),
            other => job.input = Some(other.to_owned()),
        }
    }
    Ok(Some(job))
}

fn read_value(args: &[String], index: &mut usize, option: &str) -> Result<String, String> {
    let value = args.get(*index).cloned().ok_or_else(|| format!("{option} needs a value"))?;
    *index += 1;
    Ok(value)
}

fn read_format(args: &[String], index: &mut usize, option: &str) -> Result<Format, String> {
    let name = read_value(args, index, option)?;
    Format::parse(&name).ok_or_else(|| format!("unknown format for {option}: {name}"))
}

/// Guess the format of a piece of text: markup starts with `<`, everything else is LaTeX.
fn detect_format(text: &str) -> Format {
    let trimmed = text.trim_start();
    if !trimmed.starts_with('<') {
        return Format::Latex;
    }
    if trimmed.contains("oMath") || trimmed.contains("m:r") || trimmed.contains("m:f") {
        Format::Omml
    } else {
        Format::MathMl
    }
}

/// Read an equation from bytes, reporting parse errors with enough context to fix them.
///
/// Text formats are decoded as UTF-8; the stream format is read as-is.
pub(crate) fn read_math(bytes: &[u8], from: Format) -> Result<mt_core::Math, String> {
    match from {
        Format::Latex => {
            let text = as_text(bytes)?;
            mt_tex::parse(text).map_err(|error| format!("LaTeX error at byte {}: {}", error.offset, error.message))
        }
        Format::MathMl => mt_mathml::parse(as_text(bytes)?).map_err(|error| format!("MathML error: {error}")),
        Format::Omml => mt_omml::parse(as_text(bytes)?).map_err(|error| format!("OMML error: {error}")),
        Format::Mtef => mt_mtef::parse(bytes).map_err(|error| format!("equation stream error: {error}")),
        Format::Svg | Format::Png | Format::Eps => {
            Err(format!("{} cannot be read as an equation", from.name()))
        }
    }
}

fn as_text(bytes: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(bytes).map_err(|error| format!("input is not valid UTF-8 text: {error}"))
}

/// Largest PNG this program will allocate: 128 Mpixels, about 512 MiB of RGBA.
///
/// Without this bound a huge input times a large `?scale=` lays the equation out into a bounds
/// box whose pixel area allocates tens of gigabytes and aborts the process.
const MAX_RENDER_PIXELS: u64 = 1 << 27;

/// Refuse an image whose pixel area would exceed [`MAX_RENDER_PIXELS`].
fn check_render_pixels(width: u32, height: u32) -> Result<(), String> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_RENDER_PIXELS {
        Err(format!(
            "the equation would render as {width}x{height} pixels ({pixels} total), over the {MAX_RENDER_PIXELS}-pixel limit; lower the scale or shorten the equation"
        ))
    } else {
        Ok(())
    }
}


/// Lay the equation out under the full preset `style` and produce one of the image formats.
fn render_image_with_style(
    math: &mt_core::Math,
    to: Format,
    style: &LayoutStyle,
    scale: f64,
) -> Result<Vec<u8>, String> {
    let metrics = FaceMetrics::parse(MATH_FONT, 0).ok_or("bundled math font could not be parsed")?;
    let boxes = layout(math, &metrics, style);
    match to {
        Format::Svg => Ok(to_svg(&boxes, style).into_bytes()),
        Format::Png => {
            let options = mt_render::RenderOptions { scale, ..Default::default() };
            // The bounds decide the allocation inside `render_png`, so measure them first and
            // refuse the ones that would exhaust the address space.
            let (width, height) = mt_render::image_size(&boxes, style, metrics.face(), &options);
            check_render_pixels(width, height)?;
            mt_render::render_png(&boxes, style, metrics.face(), &options)
                .map_err(|error| format!("cannot rasterise: {error}"))
        }
        Format::Eps => Ok(mt_render::render_eps(&boxes, style, metrics.face(), &mt_render::RenderOptions::default())
            .into_bytes()),
        _ => Err(format!("{} is not an image format", to.name())),
    }
}

/// Write an equation out in the requested format.
fn write_math(math: &mt_core::Math, to: Format, size: f64, scale: f64) -> Result<Vec<u8>, String> {
    write_math_with_style(math, to, &LayoutStyle::display(size), scale)
}

/// Write an equation out under the full preset `style`, so exports follow the active preset.
fn write_math_with_style(
    math: &mt_core::Math,
    to: Format,
    style: &LayoutStyle,
    scale: f64,
) -> Result<Vec<u8>, String> {
    Ok(match to {
        Format::Latex => mt_tex::to_latex(math).into_bytes(),
        Format::MathMl => mt_mathml::write(math).into_bytes(),
        Format::Omml => mt_omml::write(math).into_bytes(),
        Format::Mtef => mt_mtef::write(math),
        Format::Svg | Format::Png | Format::Eps => render_image_with_style(math, to, style, scale)?,
    })
}

fn execute(job: Job) -> Result<(), String> {
    let bytes = match (&job.input, &job.input_file) {
        (Some(text), _) => text.clone().into_bytes(),
        (None, Some(path)) => {
            std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?
        }
        (None, None) => return Err("no input given: pass the text or use --in <file>".to_owned()),
    };
    let from = job.from.unwrap_or_else(|| detect_format(&String::from_utf8_lossy(&bytes)));
    let math = read_math(&bytes, from)?;
    let produced = write_math(&math, job.to, job.size, job.scale)?;
    match &job.out {
        Some(path) => std::fs::write(path, &produced)
            .map_err(|error| format!("cannot write {}: {error}", path.display())),
        None => {
            use std::io::Write;
            std::io::stdout()
                .write_all(&produced)
                .map_err(|error| format!("cannot write to standard output: {error}"))
        }
    }
}

struct App {
    editor: Editor,
    metrics: FaceMetrics,
    style: LayoutStyle,
    /// The presets on offer: the built-ins plus any imported from files.
    presets: Vec<StylePreset>,
    /// Which entry of `presets` the style comes from.
    preset_index: usize,
    /// The formula library the side panel searches and inserts from.
    library: library::Library,
    /// Name the next library save will use.
    library_name: String,
    /// Substring the library list is filtered by.
    library_query: String,
    /// Path the preset import/export buttons act on.
    preset_path: String,
    /// Path the library import/export buttons act on.
    library_path: String,
    tex_input: String,
    status: String,
    /// LaTeX parse failure to point at inside the input box: message and byte offset.
    parse_error: Option<ParseFailure>,
    /// True while the exit-confirmation prompt is up.
    exit_prompt: bool,
    /// File the equation is read from and written back to, when opened from a document object.
    path: Option<std::path::PathBuf>,
    dirty: bool,
    /// True while the export-web dialog is up, with its two path fields.
    web_dialog: bool,
    web_docx: String,
    web_out: String,
}

/// A parse failure worth showing where it happened: the message and the byte offset.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ParseFailure {
    message: String,
    /// Byte offset into the input the failure came from.
    offset: usize,
}

/// One phase of a canvas pointer gesture, as the editor model understands it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PointerPhase {
    /// Button just went down: place the caret and start a fresh selection from there.
    Press,
    /// Button still down: extend the selection to the pointer.
    Drag,
    /// Button released without a drag: plain click.
    Click,
}

/// Font choices offered in the style panel, with their toolbar labels.
const FONT_CHOICES: [(Font, &str); 6] = [
    (Font::Italic, "变量体"),
    (Font::Upright, "正体"),
    (Font::Bold, "粗体"),
    (Font::BoldItalic, "粗斜体"),
    (Font::Script, "花体"),
    (Font::SansSerif, "无衬线"),
];

/// Size presets offered in the style panel, matching [`MathStyle`]'s four tiers.
const STYLE_CHOICES: [(MathStyle, &str); 4] = [
    (MathStyle::Full, "标准"),
    (MathStyle::Text, "文本"),
    (MathStyle::Script, "一级小"),
    (MathStyle::ScriptScript, "二级小"),
];

fn math_style_name(style: MathStyle) -> &'static str {
    STYLE_CHOICES.iter().find(|(s, _)| *s == style).map(|(_, name)| *name).unwrap_or("?")
}

/// Apply a font to the selection (or record it for the next keystroke), reporting a math change.
fn apply_font_choice(editor: &mut Editor, font: Font) -> bool {
    let selected = editor.selection().is_some();
    editor.apply_font(font);
    selected
}

/// Apply a size preset to the selection (or record it for the next insertion), reporting a change.
fn apply_style_choice(editor: &mut Editor, style: MathStyle) -> bool {
    let selected = editor.selection().is_some();
    editor.apply_style(style);
    selected
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        let metrics = FaceMetrics::parse(MATH_FONT, 0).expect("bundled math font must parse");
        let presets = mt_style_preset::builtins().to_vec();
        let style = presets[0].layout_style();
        let library = library::Library::load_default().unwrap_or_else(|error| {
            let mut empty = library::Library::default();
            empty.path = library::default_path();
            eprintln!("公式库无法读取：{error}");
            empty
        });
        let library_path = library.path.display().to_string();
        Self {
            editor: Editor::new(),
            metrics,
            style,
            presets,
            preset_index: 0,
            library,
            library_name: String::new(),
            library_query: String::new(),
            preset_path: "preset.ini".to_owned(),
            library_path,
            tex_input: String::from(r"\frac{a}{b} + \sqrt{x_i^2}"),
            status: "输入字符或点选面板插入模板；左右方向键在槽位间移动".to_owned(),
            path: None,
            dirty: false,
            parse_error: None,
            exit_prompt: false,
            web_dialog: false,
            web_docx: String::new(),
            web_out: String::new(),
        }
    }

    /// Open on the equation stored in `path`; the file is rewritten whenever the user saves.
    fn editing(cc: &eframe::CreationContext<'_>, path: std::path::PathBuf) -> Self {
        let mut app = Self::new(cc);
        app.status = match std::fs::read(&path) {
            Ok(bytes) => match read_math(&bytes, detect_format(&String::from_utf8_lossy(&bytes))) {
                Ok(math) => {
                    app.editor.set_math(math);
                    format!("已打开 {}", path.display())
                }
                Err(message) => format!("{} 无法解析：{message}", path.display()),
            },
            Err(error) => format!("无法读取 {}：{error}", path.display()),
        };
        app.path = Some(path);
        app
    }

    /// Write the equation back to the file it was opened from.
    fn save(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let text = mt_tex::to_latex(self.editor.math());
        match std::fs::write(&path, &text) {
            Ok(()) => {
                self.dirty = false;
                self.status = format!("已保存到 {}：{text}", path.display());
            }
            Err(error) => self.status = format!("保存失败：{error}"),
        }
    }

    /// Switch to `index`-th preset: the layout style is rebuilt, so canvas and exports re-typeset
    /// with the new sizes, spacing and font slots on the next frame.
    fn apply_preset(&mut self, index: usize) {
        let Some(preset) = self.presets.get(index) else { return };
        self.preset_index = index;
        self.style = preset.layout_style();
        self.editor.set_typing_font(preset.styles.variable);
        self.status = format!(
            "已切换预设「{}」：全号 {}pt，变量 {}，公式将按新字体与字号重新排版",
            preset.name,
            preset.sizes.full,
            font_name(preset.styles.variable)
        );
    }

    /// Read a preset file into the list and switch to it.
    fn import_preset(&mut self) {
        let path = self.preset_path.trim().to_owned();
        let read = std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| mt_style_preset::parse(&text).map_err(|error| error.to_string()));
        match read {
            Ok(preset) => {
                self.status = format!("已导入预设「{}」（{}）", preset.name, path);
                self.presets.push(preset);
                self.apply_preset(self.presets.len() - 1);
            }
            Err(message) => self.status = format!("导入预设失败：{message}"),
        }
    }

    /// Write the current preset to the path the panel names.
    fn export_preset(&mut self) {
        let preset = &self.presets[self.preset_index];
        let path = std::path::PathBuf::from(self.preset_path.trim());
        match std::fs::write(&path, preset.to_text()) {
            Ok(()) => self.status = format!("已导出预设「{}」到 {}", preset.name, path.display()),
            Err(error) => self.status = format!("导出预设失败：{error}"),
        }
    }

    /// Save the current equation into the library under the name the panel names.
    fn save_to_library(&mut self) {
        let latex = mt_tex::to_latex(self.editor.math());
        self.library.add(&self.library_name, &latex);
        match self.library.save() {
            Ok(()) => {
                let name = self.library.entries.last().expect("just added").name.clone();
                self.status = format!("已保存到公式库：{name}");
                self.library_name.clear();
            }
            Err(message) => self.status = format!("公式库保存失败：{message}"),
        }
    }

    /// Load a whole library file into the panel.
    fn import_library(&mut self) {
        match library::Library::load(self.library_path.trim()) {
            Ok(loaded) => {
                self.status =
                    format!("已导入公式库（{} 条）：{}", loaded.entries.len(), loaded.path.display());
                self.library = loaded;
            }
            Err(error) => self.status = format!("导入公式库失败：{error}"),
        }
    }

    /// Write the library to the path the panel names.
    fn export_library(&mut self) {
        let path = std::path::PathBuf::from(self.library_path.trim());
        match self.library.save_to(&path) {
            Ok(()) => self.status = format!("公式库已另存到 {}", path.display()),
            Err(message) => self.status = format!("公式库另存失败：{message}"),
        }
    }

    /// Insert one library entry at the caret; used by the list buttons and by canvas drops.
    fn insert_entry(&mut self, entry: &library::LibraryEntry) {
        match library::insert_entry(&mut self.editor, entry) {
            Ok(()) => {
                self.dirty = true;
                self.status = format!("已插入「{}」", entry.name);
            }
            Err(message) => self.status = format!("插入「{}」失败：{message}", entry.name),
        }
    }

    /// The preset and formula-library side panel.
    fn draw_side_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("样式预设");
        let mut selected = self.preset_index;
        egui::ComboBox::from_id_salt("preset-select")
            .selected_text(&self.presets[selected].name)
            .show_ui(ui, |ui| {
                for (index, preset) in self.presets.iter().enumerate() {
                    ui.selectable_value(&mut selected, index, preset.name.clone());
                }
            });
        if selected != self.preset_index {
            self.apply_preset(selected);
        }
        ui.weak(preset_status(&self.presets[self.preset_index]));
        ui.horizontal(|ui| {
            ui.label("预设文件");
            ui.add(egui::TextEdit::singleline(&mut self.preset_path).desired_width(120.0));
        });
        ui.horizontal(|ui| {
            if ui.button("导入预设").clicked() {
                self.import_preset();
            }
            if ui.button("导出预设").clicked() {
                self.export_preset();
            }
        });

        ui.separator();
        ui.heading("公式库");
        ui.horizontal(|ui| {
            ui.label("名称");
            ui.add(egui::TextEdit::singleline(&mut self.library_name).desired_width(120.0));
            if ui.button("保存当前公式").clicked() {
                self.save_to_library();
            }
        });
        ui.horizontal(|ui| {
            ui.label("检索");
            ui.add(
                egui::TextEdit::singleline(&mut self.library_query)
                    .desired_width(120.0)
                    .hint_text("名称或内容"),
            );
        });
        let hits = self.library.search(&self.library_query);
        egui::ScrollArea::vertical().show_rows(ui, 22.0, hits.len(), |ui, range| {
            for index in &hits[range] {
                let entry = self.library.entries[*index].clone();
                ui.horizontal(|ui| {
                    let response = ui
                        .add(egui::Button::new(&entry.name).sense(egui::Sense::click_and_drag()))
                        .on_hover_text(&entry.latex);
                    if response.clicked() {
                        self.insert_entry(&entry);
                    }
                    if response.dragged() {
                        egui::DragAndDrop::set_payload(ui.ctx(), entry.clone());
                    }
                    if ui.small_button("删").clicked() {
                        self.library.remove(*index);
                        let _ = self.library.save();
                    }
                });
                ui.weak(&entry.latex);
            }
        });
        ui.horizontal(|ui| {
            ui.label("库文件");
            ui.add(egui::TextEdit::singleline(&mut self.library_path).desired_width(120.0));
        });
        ui.horizontal(|ui| {
            if ui.button("导入库").clicked() {
                self.import_library();
            }
            if ui.button("另存库").clicked() {
                self.export_library();
            }
        });
    }

    fn apply_palette_item(&mut self, item: PaletteItem) {
        self.dirty = true;
        match item {
            PaletteItem::Symbol { ch, class, font } => self.editor.insert_symbol(ch, class, font),
            PaletteItem::Template(template) => self.editor.insert_template(template),
            PaletteItem::Space(space) => self.editor.insert_space(space),
        }
    }

    /// Import whatever the input box holds, picking the format from the text itself.
    ///
    /// LaTeX failures keep their byte offset so the input area can point at the offending spot.
    fn import_text(&mut self) {
        let text = self.tex_input.clone();
        let from = detect_format(&text);
        if from == Format::Latex {
            match mt_tex::parse(&text) {
                Ok(math) => {
                    self.editor.set_math(math);
                    self.dirty = true;
                    self.parse_error = None;
                    self.status = "已导入（按 LaTeX 解析）".to_owned();
                }
                Err(error) => {
                    self.parse_error = Some(ParseFailure {
                        message: error.message.clone(),
                        offset: error.offset,
                    });
                    self.status = format!(
                        "LaTeX 解析失败：{}（第 {} 字节处）",
                        error.message, error.offset
                    );
                }
            }
            return;
        }
        match read_math(text.as_bytes(), from) {
            Ok(math) => {
                self.editor.set_math(math);
                self.dirty = true;
                self.parse_error = None;
                self.status = format!("已导入（按 {} 解析）", from.name());
            }
            Err(message) => {
                self.parse_error = None;
                self.status = format!("导入失败：{message}");
            }
        }
    }

    /// The equation's bytes in `format`, following the active preset.
    ///
    /// A stream export declares the preset as its equation preferences, so a MathType reader
    /// re-typesets the equation with the same sizes and style slots.
    fn preset_bytes(&self, format: Format) -> Result<Vec<u8>, String> {
        if format == Format::Mtef {
            return Ok(mt_mtef::write_with_preferences(
                self.editor.math(),
                &mt_style_preset::to_preferences(&self.presets[self.preset_index]),
            ));
        }
        write_math_with_style(self.editor.math(), format, &self.style, 3.0)
    }

    /// Put the equation on the clipboard in one of the interchange formats.
    fn copy_as(&mut self, ctx: &egui::Context, format: Format) {
        match self.preset_bytes(format) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                ctx.copy_text(text.clone());
                self.status = format!("已复制 {}（{} 字节）", format.name(), text.len());
            }
            Err(message) => self.status = format!("复制失败：{message}"),
        }
    }

    /// Show what the input box currently parses to, before it is committed to the canvas.
    ///
    /// LaTeX that does not parse yet simply shows nothing; the status line carries the error.
    fn draw_preview(&self, ui: &mut egui::Ui) {
        if detect_format(&self.tex_input) != Format::Latex {
            return;
        }
        let Ok(math) = mt_tex::parse(&self.tex_input) else {
            return;
        };
        let mut style = self.style;
        style.size = style.size.min(16.0);
        let boxes = layout(&math, &self.metrics, &style);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), Sense::hover());
        let painter = ui.painter_at(rect);
        let origin = Pos2::new(rect.left() + 8.0, rect.center().y);
        let font_id = |size: f64| FontId::new(size as f32, FontFamily::Name(MATH_FAMILY.into()));
        draw_boxes(&painter, &boxes, origin, &self.metrics, ui.visuals().weak_text_color(), &font_id);
    }

    /// Write the equation to a file in the requested image format.
    fn export(&mut self, format: Format) {
        match self.preset_bytes(format) {
            Ok(bytes) => {
                let path = format!("equation.{}", format.name());
                match std::fs::write(&path, &bytes) {
                    Ok(()) => self.status = format!("已写出 {path}（{} 字节）", bytes.len()),
                    Err(error) => self.status = format!("写出 {path} 失败：{error}"),
                }
            }
            Err(message) => self.status = format!("导出失败：{message}"),
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        // A focused text field (the LaTeX box) owns the keyboard while it is active.
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let events = ctx.input(|input| input.events.clone());
        for event in events {
            match event {
                egui::Event::Text(text) => {
                    for ch in text.chars() {
                        if self.editor.insert_char(ch) {
                            self.dirty = true;
                        }
                    }
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => match key {
                    egui::Key::Backspace => {
                        self.dirty |= self.editor.backspace();
                    }
                    egui::Key::Delete => {
                        self.dirty |= self.editor.delete();
                    }
                    egui::Key::S if modifiers.command => self.save(),
                    egui::Key::ArrowLeft => self.editor.move_left(),
                    egui::Key::ArrowRight => self.editor.move_right(),
                    egui::Key::ArrowUp => self.editor.move_vertical(true),
                    egui::Key::ArrowDown => self.editor.move_vertical(false),
                    egui::Key::Tab => self.editor.tab(!modifiers.shift),
                    egui::Key::Home => self.editor.move_line_start(),
                    egui::Key::End => self.editor.move_line_end(),
                    egui::Key::Z if modifiers.command => {
                        if modifiers.shift {
                            self.dirty |= self.editor.redo();
                        } else {
                            self.dirty |= self.editor.undo();
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    /// Lay the current equation out once per frame; drawing and hit testing share the result.
    fn geometry(&self) -> (LayoutBox, Vec<RowAnchor>) {
        layout_with_anchors(self.editor.math(), &self.metrics, &self.style)
    }

    /// Draw the insertion point where the caret currently sits.
    fn draw_caret(&self, painter: &egui::Painter, origin: Pos2, anchors: &[RowAnchor], color: Color32) {
        let cursor = self.editor.cursor();
        let Some(anchor) = anchors.iter().find(|anchor| &anchor.path == &cursor.path) else {
            return;
        };
        let last = anchor.offsets.len().saturating_sub(1);
        let offset = anchor.offsets[cursor.index.min(last)];
        let x = origin.x + (anchor.origin.0 + offset) as f32;
        let baseline = origin.y + anchor.origin.1 as f32;
        let size = self.style.size as f32;
        let caret = Rect::from_min_max(
            Pos2::new(x, baseline - size * 0.85),
            Pos2::new(x + 1.5, baseline + size * 0.25),
        );
        painter.rect_filled(caret, 0.0, color);
    }

    /// Cover the selected span of the caret's row with a translucent highlight rectangle.
    fn draw_selection(
        &self,
        painter: &egui::Painter,
        origin: Pos2,
        anchors: &[RowAnchor],
        color: Color32,
    ) {
        let Some((start, end)) = self.editor.selection() else {
            return;
        };
        let cursor = self.editor.cursor();
        let Some(anchor) = anchors.iter().find(|anchor| &anchor.path == &cursor.path) else {
            return;
        };
        let last = anchor.offsets.len().saturating_sub(1);
        let x0 = origin.x + (anchor.origin.0 + anchor.offsets[start.min(last)]) as f32;
        let x1 = origin.x + (anchor.origin.0 + anchor.offsets[end.min(last)]) as f32;
        let baseline = origin.y + anchor.origin.1 as f32;
        let size = self.style.size as f32;
        let span = Rect::from_min_max(
            Pos2::new(x0, baseline - size * 0.85),
            Pos2::new(x1, baseline + size * 0.25),
        );
        painter.rect_filled(span, 2.0, color);
    }

    /// Modal asking whether to discard unsaved changes; shown when a close was cancelled.
    /// The export-web dialog: a .docx path in, an output directory out. The editor works on one
    /// equation, so the document to export is named here rather than opened.
    fn show_web_dialog(&mut self, ctx: &egui::Context) {
        egui::Window::new("导出网页")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("把整个 Word 文档导出为网页：公式转为 MathML，图片写入 media/ 目录。");
                ui.horizontal(|ui| {
                    ui.label("文档");
                    ui.text_edit_singleline(&mut self.web_docx);
                });
                ui.horizontal(|ui| {
                    ui.label("输出目录");
                    ui.text_edit_singleline(&mut self.web_out);
                });
                ui.horizontal(|ui| {
                    if ui.button("导出").clicked() {
                        self.run_web_export();
                    }
                    if ui.button("取消").clicked() {
                        self.web_dialog = false;
                    }
                });
            });
    }

    /// Runs the export named in the dialog; the status line carries the outcome.
    fn run_web_export(&mut self) {
        let input = std::path::PathBuf::from(self.web_docx.trim());
        let outdir = self.web_out.trim().to_owned();
        let result = (|| -> Result<std::path::PathBuf, String> {
            if input.as_os_str().is_empty() {
                return Err("请先填写文档路径".to_owned());
            }
            let outdir = if outdir.is_empty() {
                input.with_extension("").to_string_lossy().into_owned() + "-web"
            } else {
                outdir
            };
            let outdir = std::path::PathBuf::from(outdir);
            let bytes = std::fs::read(&input)
                .map_err(|error| format!("无法读取 {}: {error}", input.display()))?;
            let document = mt_docx::Document::open(&bytes)
                .map_err(|error| format!("无法打开 {}: {error}", input.display()))?;
            mt_html::export_files(&document, &mt_html::Options::default(), &outdir)
        })();
        match result {
            Ok(page) => {
                self.status = format!("已导出 {}", page.display());
                self.web_dialog = false;
            }
            Err(message) => self.status = format!("导出网页失败：{message}"),
        }
    }

    fn show_exit_prompt(&mut self, ctx: &egui::Context) {
        egui::Window::new("未保存的修改")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("公式有未保存的修改，直接退出将丢失这些修改。");
                ui.horizontal(|ui| {
                    if ui.button("放弃修改并退出").clicked() {
                        self.exit_prompt = false;
                        ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                    }
                    if ui.button("继续编辑").clicked() {
                        self.exit_prompt = false;
                    }
                });
            });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Standalone sessions ask before discarding unsaved work; hand-off (--edit) sessions
        // always write the file back on exit, so closing is unconditional there.
        if ui.ctx().input(|input| input.viewport().close_requested())
            && self.path.is_none()
            && self.dirty
            && !self.exit_prompt
        {
            ui.ctx().send_viewport_cmd(ViewportCommand::CancelClose);
            self.exit_prompt = true;
        }
        if self.exit_prompt {
            self.show_exit_prompt(ui.ctx());
        }
        if self.web_dialog {
            self.show_web_dialog(ui.ctx());
        }
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for palette in palettes() {
                    ui.menu_button(palette.name, |ui| {
                        ui.horizontal_top(|ui| {
                            for column in palette.columns {
                                ui.vertical(|ui| {
                                    for item in *column {
                                        let button = ui.button(item.label());
                                        if button.clicked() {
                                            self.apply_palette_item(*item);
                                            ui.close();
                                        }
                                    }
                                });
                            }
                        });
                    });
                }
                ui.separator();
                if ui.button("撤销").clicked() {
                    self.editor.undo();
                }
                if ui.button("重做").clicked() {
                    self.editor.redo();
                }
                ui.separator();
                let shown_font = current_font(&self.editor);
                for (font, name) in FONT_CHOICES {
                    if ui.selectable_label(shown_font == font, name).clicked() {
                        self.dirty |= apply_font_choice(&mut self.editor, font);
                    }
                }
                ui.separator();
                let shown_style = self.editor.typing_style();
                for (style, name) in STYLE_CHOICES {
                    if ui.selectable_label(shown_style == Some(style), name).clicked() {
                        self.dirty |= apply_style_choice(&mut self.editor, style);
                    }
                }
                ui.separator();
                ui.add(
                    egui::Slider::new(&mut self.style.size, 8.0..=48.0)
                        .text("显示字号")
                        .fixed_decimals(0),
                );
                if ui.button("复制 LaTeX").clicked() {
                    self.copy_as(ui.ctx(), Format::Latex);
                }
                if ui.button("复制 MathML").clicked() {
                    self.copy_as(ui.ctx(), Format::MathMl);
                }
                if ui.button("复制 OMML").clicked() {
                    self.copy_as(ui.ctx(), Format::Omml);
                }
                if ui.button("导出 SVG").clicked() {
                    self.export(Format::Svg);
                }
                if ui.button("导出 PNG").clicked() {
                    self.export(Format::Png);
                }
                if ui.button("导出 EPS").clicked() {
                    self.export(Format::Eps);
                }
                if ui.button("导出网页…").clicked() {
                    self.web_dialog = true;
                }
                if self.path.is_some() {
                    let label = if self.dirty { "保存 *" } else { "保存" };
                    if ui.button(label).clicked() {
                        self.save();
                    }
                }
            });
        });
        egui::Panel::right("presets").resizable(true).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.draw_side_panel(ui));
        });

        egui::CentralPanel::default().show(ui, |ui| {
            // Bottom-up: the footer claims its natural height first, the canvas then takes whatever
            // is left, so neither depends on a hand-computed height.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                self.draw_preview(ui);
                ui.label(&self.status);
                if let Some(failure) = &self.parse_error {
                    let marker = error_marker(&self.tex_input, failure.offset);
                    ui.monospace(
                        egui::RichText::new(marker).color(ui.visuals().error_fg_color).family(egui::FontFamily::Monospace),
                    );
                }
                ui.weak(style_status(&self.editor));
                ui.horizontal(|ui| {
                    ui.label("输入");
                    let input = egui::TextEdit::singleline(&mut self.tex_input).desired_width(520.0);
                    let input_response = ui.add(input);
                    let convert = ui.button("导入公式").clicked();
                    let submitted = input_response.lost_focus()
                        && ui.input(|state| state.key_pressed(egui::Key::Enter));
                    if convert || submitted {
                        self.import_text();
                    }
                });
                ui.separator();

                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
                let painter = ui.painter_at(rect);
                painter.rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
                    egui::StrokeKind::Inside,
                );
                let color = ui.visuals().text_color();
                let (boxes, anchors) = self.geometry();
                let origin = match self.presets[self.preset_index].alignment {
                    Side::Left => Pos2::new(rect.left() + 24.0, rect.center().y),
                    Side::Center => {
                        Pos2::new(rect.center().x - boxes.width() as f32 / 2.0, rect.center().y)
                    }
                    Side::Right => {
                        Pos2::new(rect.right() - 24.0 - boxes.width() as f32, rect.center().y)
                    }
                };
                let font_id = |size: f64| FontId::new(size as f32, FontFamily::Name(MATH_FAMILY.into()));
                self.draw_selection(&painter, origin, &anchors, ui.visuals().selection.bg_fill);
                draw_boxes(&painter, &boxes, origin, &self.metrics, color, &font_id);
                self.draw_caret(&painter, origin, &anchors, color);

                let phase = if response.drag_started() {
                    Some(PointerPhase::Press)
                } else if response.dragged() {
                    Some(PointerPhase::Drag)
                } else if response.clicked() {
                    Some(PointerPhase::Click)
                } else {
                    None
                };
                if let Some(phase) = phase {
                    if let Some(point) = response.interact_pointer_pos() {
                        apply_pointer(&mut self.editor, &anchors, origin, point, phase);
                    }
                }
                // A library entry dragged onto the canvas inserts where it lands.
                if let Some(entry) = egui::DragAndDrop::payload::<library::LibraryEntry>(ui.ctx()) {
                    if ui.input(|state| state.pointer.any_released()) {
                        if response.hovered() {
                            self.insert_entry(&entry);
                        }
                        egui::DragAndDrop::clear_payload(ui.ctx());
                    }
                }
            });

            self.handle_keys(ui.ctx());
        });
    }

    /// The object server reads the file back once this process exits, so write on the way out.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.dirty {
            self.save();
        }
    }
}

/// Beyond this weighted distance a point counts as blank canvas rather than a caret position,
/// so clicking far from the equation drops the selection instead of teleporting the caret.
const MAX_HIT_COST: f32 = 600.0;

/// Nearest insertion position to `point`, weighted so that staying on the caret's own line wins
/// over a slightly smaller horizontal distance on a line further away.
fn caret_at(anchors: &[RowAnchor], origin: Pos2, point: Pos2) -> Option<Cursor> {
    let mut best: Option<(f32, Cursor)> = None;
    for anchor in anchors {
        let baseline = origin.y + anchor.origin.1 as f32;
        for (index, offset) in anchor.offsets.iter().enumerate() {
            let x = origin.x + (anchor.origin.0 + offset) as f32;
            let cost = (point.x - x).abs() + (point.y - baseline).abs() * 8.0;
            if cost <= MAX_HIT_COST && best.as_ref().is_none_or(|(best_cost, _)| cost < *best_cost) {
                best = Some((cost, Cursor { path: anchor.path.clone(), index }));
            }
        }
    }
    best.map(|(_, cursor)| cursor)
}

/// Feed one phase of a canvas pointer gesture to the editor model.
///
/// A press re-anchors the selection where it lands; a drag extends it to the pointer; a plain
/// click just moves the caret. A miss (blank canvas) never moves the caret — it only drops the
/// selection. Returns whether the gesture is still in progress (press or drag).
fn apply_pointer(
    editor: &mut Editor,
    anchors: &[RowAnchor],
    origin: Pos2,
    point: Pos2,
    phase: PointerPhase,
) -> bool {
    match phase {
        PointerPhase::Press => {
            match caret_at(anchors, origin, point) {
                Some(target) => {
                    editor.set_cursor(target.clone());
                    editor.select_to(target);
                }
                None => editor.clear_selection(),
            }
            true
        }
        PointerPhase::Drag => {
            if let Some(target) = caret_at(anchors, origin, point) {
                editor.extend_to(target);
            }
            true
        }
        PointerPhase::Click => {
            match caret_at(anchors, origin, point) {
                Some(target) => editor.set_cursor(target),
                None => editor.clear_selection(),
            }
            false
        }
    }
}

/// The failing line of `text` with a caret under byte `offset`, for the status area.
fn error_marker(text: &str, offset: usize) -> String {
    let mut consumed = 0;
    for line in text.lines() {
        let end = consumed + line.len();
        if offset <= end {
            let column = text[consumed..offset.min(end)].chars().count();
            return format!("{line}\n{:<width$}^", "", width = column);
        }
        consumed = end + 1;
    }
    text.to_owned()
}

/// Explicit font of a node, when it carries one.
fn node_font(node: &mt_core::Node) -> Option<Font> {
    match node {
        mt_core::Node::Char(char) => Some(char.font),
        mt_core::Node::Styled(styled) => styled.font,
        _ => None,
    }
}

/// Font the style panel should highlight: the first selected node's when a selection exists,
/// otherwise the font the next keystroke will use.
fn current_font(editor: &Editor) -> Font {
    if let Some((start, _)) = editor.selection() {
        if let Some(mt_core::Node::Row(row)) = editor.math().node_at(&editor.cursor().path) {
            if let Some(font) = row.iter().nth(start).and_then(node_font) {
                return font;
            }
        }
    }
    editor.typing_font()
}

fn font_name(font: Font) -> &'static str {
    FONT_CHOICES.iter().find(|(f, _)| *f == font).map(|(_, name)| *name).unwrap_or("其他字体")
}

/// One-line style state for the status area: what a style command will act on.
fn style_status(editor: &Editor) -> String {
    let scope = if editor.selection().is_some() { "选区" } else { "输入" };
    let style = editor.typing_style().map(math_style_name).unwrap_or("继承");
    format!("{scope}样式：字体 {}，字号 {style}", font_name(current_font(editor)))
}

/// One-line preset summary for the side panel.
fn preset_status(preset: &StylePreset) -> String {
    let alignment = match preset.alignment {
        Side::Left => "左对齐",
        Side::Center => "居中",
        Side::Right => "右对齐",
    };
    format!(
        "全号 {}pt · 脚本 {}pt / {}pt · 变量 {} · {}",
        preset.sizes.full,
        preset.sizes.script,
        preset.sizes.script_script,
        font_name(preset.styles.variable),
        alignment
    )
}

/// Register the math face, plus a system face for the CJK interface labels when one loads.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert(MATH_FAMILY.to_owned(), Arc::new(FontData::from_static(MATH_FONT)));
    fonts
        .families
        .entry(FontFamily::Name(MATH_FAMILY.into()))
        .or_default()
        .push(MATH_FAMILY.to_owned());

    for candidate in UI_FONT_CANDIDATES {
        let Ok(bytes) = std::fs::read(candidate) else {
            continue;
        };
        // Only hand egui font data we know it can parse; a bad face would take the whole UI down.
        if ttf_parser::Face::parse(&bytes, 0).is_err() {
            continue;
        }
        fonts.font_data.insert("ui-cjk".to_owned(), Arc::new(FontData::from_owned(bytes)));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts.families.entry(family).or_default().push("ui-cjk".to_owned());
        }
        break;
    }

    ctx.set_fonts(fonts);
}

/// Paint every leaf of a laid-out equation. Coordinates are absolute in points, `y` downwards
/// from the equation's baseline.
fn draw_boxes(
    painter: &egui::Painter,
    node: &LayoutBox,
    origin: Pos2,
    metrics: &FaceMetrics,
    color: Color32,
    font_id: &dyn Fn(f64) -> FontId,
) {
    match node {
        LayoutBox::HBox { items, .. } | LayoutBox::VBox { items, .. } => {
            for item in items {
                draw_boxes(painter, item, origin, metrics, color, font_id);
            }
        }
        LayoutBox::Glyph { ch, font, x, y, size, yscale } => {
            let size = size * yscale;
            let position = origin + Vec2::new(*x as f32, (*y - metrics.ascender() * size) as f32);
            let _ = font;
            painter.text(position, Align2::LEFT_TOP, ch, font_id(size), color);
        }
        LayoutBox::Rule { x, y, width, height, depth, dashed } => {
            let top = origin + Vec2::new(*x as f32, (*y - *height) as f32);
            let bottom = origin + Vec2::new((*x + *width) as f32, (*y + *depth) as f32);
            let rect = Rect::from_min_max(top, bottom);
            if *dashed {
                // An empty argument slot: outline it so the author sees where input goes.
                let stroke = Stroke::new(1.0, color.gamma_multiply(0.35));
                dashed_rect(painter, rect, stroke);
            } else {
                painter.rect_filled(rect, 0.0, color);
            }
        }
        LayoutBox::Kern { .. } => {}
    }
}

fn dashed_rect(painter: &egui::Painter, rect: Rect, stroke: Stroke) {
    const DASH: f32 = 3.0;
    let mut x = rect.left();
    while x < rect.right() {
        let end = (x + DASH).min(rect.right());
        painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(end, rect.top())], stroke);
        painter.line_segment([Pos2::new(x, rect.bottom()), Pos2::new(end, rect.bottom())], stroke);
        x += DASH * 2.0;
    }
    let mut y = rect.top();
    while y < rect.bottom() {
        let end = (y + DASH).min(rect.bottom());
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.left(), end)], stroke);
        painter.line_segment([Pos2::new(rect.right(), y), Pos2::new(rect.right(), end)], stroke);
        y += DASH * 2.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_core::{Math, Node};

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn parse_job_applies_defaults() {
        let job = parse_job(&args(&["x"])).expect("parses").expect("is a job");
        assert_eq!(job.to, Format::Svg);
        assert_eq!(job.from, None);
        assert_eq!(job.input.as_deref(), Some("x"));
        assert_eq!(job.size, 18.0);
        assert_eq!(job.scale, 3.0);
    }

    #[test]
    fn parse_job_reads_options() {
        let job = parse_job(&args(&[
            "--from", "mtef", "--to", "latex", "--size", "24", "--scale", "2", "body",
        ]))
        .expect("parses")
        .expect("is a job");
        assert_eq!(job.from, Some(Format::Mtef));
        assert_eq!(job.to, Format::Latex);
        assert_eq!(job.size, 24.0);
        assert_eq!(job.scale, 2.0);
        assert_eq!(job.input.as_deref(), Some("body"));
    }

    #[test]
    fn help_returns_no_job() {
        assert!(parse_job(&args(&["--help"])).expect("parses").is_none());
        assert!(parse_job(&args(&["-h"])).expect("parses").is_none());
    }

    #[test]
    fn parse_job_rejects_bad_arguments() {
        assert!(parse_job(&args(&["--nope"])).is_err());
        assert!(parse_job(&args(&["--from", "bogus", "x"])).is_err());
        assert!(parse_job(&args(&["--size"])).is_err());
        assert!(parse_job(&args(&["--size", "abc"])).is_err());
        assert!(parse_job(&args(&["--scale", "abc"])).is_err());
    }

    #[test]
    fn parse_job_rejects_out_of_range_size_and_scale() {
        assert!(parse_job(&args(&["--size", "9999999", "x"])).is_err());
        assert!(parse_job(&args(&["--size", "0", "x"])).is_err());
        assert!(parse_job(&args(&["--scale", "0", "x"])).is_err());
        assert!(parse_job(&args(&["--scale", "1000", "x"])).is_err());
    }

    #[test]
    fn validate_size_accepts_the_ends_of_the_range() {
        assert_eq!(validate_size(MIN_SIZE).unwrap(), MIN_SIZE);
        assert_eq!(validate_size(MAX_SIZE).unwrap(), MAX_SIZE);
        assert!(validate_size(f64::NAN).is_err());
        assert!(validate_size(f64::INFINITY).is_err());
    }

    #[test]
    fn validate_scale_accepts_the_ends_of_the_range() {
        assert_eq!(validate_scale(MIN_SCALE).unwrap(), MIN_SCALE);
        assert_eq!(validate_scale(MAX_SCALE).unwrap(), MAX_SCALE);
        assert!(validate_scale(f64::NAN).is_err());
        assert!(validate_scale(f64::INFINITY).is_err());
    }

    #[test]
    fn the_pixel_guard_refuses_only_oversized_images() {
        // An ordinary screen-sized image is fine.
        assert!(check_render_pixels(1920, 1080).is_ok());
        // Just at the limit is fine; one pixel more is not.
        let side = (MAX_RENDER_PIXELS as f64).sqrt() as u32;
        assert!(check_render_pixels(side, side).is_ok());
        assert!(check_render_pixels(side + 1, side + 1).is_err());
        let error = check_render_pixels(u32::MAX, u32::MAX).unwrap_err();
        assert!(error.contains("pixel"), "{error}");
    }

    #[test]
    fn an_equation_too_large_to_rasterise_is_refused_not_crashed() {
        // A very wide equation at the largest size and scale would once have been laid out into
        // a bounds box and then allocated as an RGBA buffer of tens of gigabytes, aborting the
        // process; a request this big must now come back as an error (the HTTP front end turns
        // it into a 400) instead.
        let math = Math::new(Node::row((0..200_000).map(|_| Node::variable('x'))));
        let error = write_math(&math, Format::Png, MAX_SIZE, MAX_SCALE).unwrap_err();
        assert!(error.contains("pixel limit"), "{error}");
    }

    #[test]
    fn a_small_equation_still_renders_as_png() {
        let math = Math::new(Node::variable('x'));
        let bytes = write_math(&math, Format::Png, 18.0, 3.0).expect("renders");
        assert!(!bytes.is_empty());
    }

    /// A pointer fixture: an editor on `ab+cd`, its layout anchors, and a point in item `i`'s
    /// middle (strictly closer to insertion point `i + 1` than to `i`).
    fn drag_fixture() -> (Editor, Vec<RowAnchor>, Box<dyn Fn(usize) -> Pos2>) {
        let editor = Editor::from_math(mt_tex::parse("ab+cd").expect("parses"));
        let metrics = FaceMetrics::parse(MATH_FONT, 0).expect("font parses");
        let style = LayoutStyle::display(18.0);
        let (_, anchors) = layout_with_anchors(editor.math(), &metrics, &style);
        let root = anchors
            .iter()
            .find(|anchor| anchor.path == mt_core::Path::root())
            .expect("root row anchored")
            .clone();
        let point = move |item: usize| {
            let mid = (root.offsets[item] + root.offsets[item + 1]) / 2.0;
            // Nudge right of the midpoint so the nearest insertion point is unambiguous.
            let x = root.origin.0 + mid + (root.offsets[item + 1] - root.offsets[item]).max(2.0) * 0.1;
            Pos2::new(x as f32, root.origin.1 as f32)
        };
        (editor, anchors, Box::new(point))
    }

    #[test]
    fn drag_press_then_move_grows_the_selection() {
        let (mut editor, anchors, point) = drag_fixture();
        apply_pointer(&mut editor, &anchors, Pos2::ZERO, point(1), PointerPhase::Press);
        assert_eq!(editor.selection(), None, "a press alone selects nothing");
        apply_pointer(&mut editor, &anchors, Pos2::ZERO, point(3), PointerPhase::Drag);
        assert_eq!(editor.selection(), Some((2, 4)), "drag covers items 2 and 3");
    }

    #[test]
    fn clicking_blank_canvas_drops_the_selection_without_moving_the_caret() {
        let (mut editor, anchors, point) = drag_fixture();
        apply_pointer(&mut editor, &anchors, Pos2::ZERO, point(1), PointerPhase::Press);
        apply_pointer(&mut editor, &anchors, Pos2::ZERO, point(3), PointerPhase::Drag);
        assert!(editor.selection().is_some());
        let before = editor.cursor().clone();
        assert!(!apply_pointer(&mut editor, &anchors, Pos2::ZERO, Pos2::new(9000.0, 9000.0), PointerPhase::Click));
        assert_eq!(editor.selection(), None, "blank click clears the selection");
        assert_eq!(editor.cursor(), &before, "blank click leaves the caret alone");
    }

    #[test]
    fn caret_at_reports_blank_canvas_as_a_miss() {
        let (_, anchors, point) = drag_fixture();
        assert!(caret_at(&anchors, Pos2::ZERO, point(0)).is_some());
        assert!(caret_at(&anchors, Pos2::ZERO, Pos2::new(9000.0, 9000.0)).is_none());
    }

    #[test]
    fn font_choice_applies_to_the_selection_and_reports_the_change() {
        let (mut editor, anchors, point) = drag_fixture();
        apply_pointer(&mut editor, &anchors, Pos2::ZERO, point(0), PointerPhase::Press);
        apply_pointer(&mut editor, &anchors, Pos2::ZERO, point(1), PointerPhase::Drag);
        assert_eq!(editor.selection(), Some((1, 2)));
        assert!(apply_font_choice(&mut editor, Font::Bold));
        assert_eq!(current_font(&editor), Font::Bold, "the panel reflects the selection");
        let latex = mt_tex::to_latex(editor.math());
        assert!(latex.contains("\\mathbf"), "{latex}");
        // With no selection the same command only arms the typing font.
        editor.set_cursor(Cursor::start());
        assert!(!apply_font_choice(&mut editor, Font::Script));
        assert_eq!(editor.typing_font(), Font::Script);
        assert!(!mt_tex::to_latex(editor.math()).contains("mathcal"));
    }

    #[test]
    fn style_choice_applies_a_size_preset_to_the_selection() {
        let (mut editor, anchors, point) = drag_fixture();
        apply_pointer(&mut editor, &anchors, Pos2::ZERO, point(1), PointerPhase::Press);
        apply_pointer(&mut editor, &anchors, Pos2::ZERO, point(2), PointerPhase::Drag);
        assert!(apply_style_choice(&mut editor, MathStyle::Script));
        assert_eq!(editor.typing_style(), Some(MathStyle::Script));
        let latex = mt_tex::to_latex(editor.math());
        assert!(latex.contains("scriptstyle"), "{latex}");
    }

    #[test]
    fn style_status_names_the_font_and_scope() {
        let editor = Editor::new();
        assert!(style_status(&editor).contains("输入"), "{}", style_status(&editor));
        assert!(style_status(&editor).contains("变量体"), "{}", style_status(&editor));
    }

    #[test]
    fn error_marker_points_at_the_offending_byte() {
        let marker = error_marker("a + \\bad{", 8);
        let mut lines = marker.lines();
        assert_eq!(lines.next(), Some("a + \\bad{"));
        assert_eq!(lines.next(), Some("        ^"), "caret sits under byte 8");
        // Offsets on later lines indent the caret accordingly.
        let marker = error_marker("x + y\n\\sqrt{", 8);
        assert_eq!(marker.lines().nth(1), Some("  ^"), "byte 8 is column 2 of line 2");
    }

    #[test]
    fn acceptance_drag_bold_template_undo_redo_keeps_every_step() {
        let (mut editor, anchors, point) = drag_fixture();
        // 1. drag-select `b`.
        apply_pointer(&mut editor, &anchors, Pos2::ZERO, point(0), PointerPhase::Press);
        apply_pointer(&mut editor, &anchors, Pos2::ZERO, point(0), PointerPhase::Drag);
        assert_eq!(editor.selection(), None, "dragging within one item selects nothing");
        apply_pointer(&mut editor, &anchors, Pos2::ZERO, point(1), PointerPhase::Drag);
        // 2. apply bold to the selection.
        assert!(apply_font_choice(&mut editor, Font::Bold));
        let bolded = mt_tex::to_latex(editor.math());
        assert!(bolded.contains("\\mathbf"), "{bolded}");
        // 3. insert a template at the caret.
        editor.insert_template(mt_edit::Template::Fraction);
        let with_template = mt_tex::to_latex(editor.math());
        assert!(with_template.contains("\\frac"), "{with_template}");
        // 4. undo returns to the bolded text; redo restores the template.
        assert!(editor.undo());
        assert_eq!(mt_tex::to_latex(editor.math()), bolded);
        assert!(editor.redo());
        assert_eq!(mt_tex::to_latex(editor.math()), with_template);
    }

    #[test]
    fn switching_presets_retypesets_the_same_tree() {
        let math = mt_tex::parse(r"\frac{a}{b} + \sqrt{x_i^2}").expect("fixture parses");
        let metrics = FaceMetrics::parse(MATH_FONT, 0).expect("bundled font parses");
        let presets = mt_style_preset::builtins();
        let width = |index: usize| layout(&math, &metrics, &presets[index].layout_style()).width();
        let (default, compact, large) = (width(0), width(2), width(3));
        assert!(compact < default, "the compact preset must tighten the same tree");
        assert!(large > default * 1.05, "the large-print preset must enlarge the same tree");
    }

    #[test]
    fn preset_status_names_sizes_and_alignment() {
        let status = preset_status(&mt_style_preset::builtins()[0]);
        assert!(status.contains("18pt"), "{status}");
        assert!(status.contains("变量体"), "{}", status);
        assert!(status.contains("居中"), "{status}");
    }

    #[test]
    fn a_stream_export_embeds_the_active_preset() {
        // The editor has no cheap constructor outside eframe, so exercise the same wiring the
        // copy path uses: the preset's preferences travel inside a written stream.
        let math = mt_tex::parse("x^2").unwrap();
        let preset = &mt_style_preset::builtins()[2];
        let stream =
            mt_mtef::write_with_preferences(&math, &mt_style_preset::to_preferences(preset));
        let read = mt_mtef::read(&stream).expect("the stream reads back");
        assert_eq!(read.header.preferences.sizes[0].value, "12", "compact preset full size");
    }
}
