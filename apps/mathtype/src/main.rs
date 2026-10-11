//! Equation editor: palette toolbar, canvas editing, format conversion.
//!
//! Three ways in:
//! - no arguments: the graphical editor (palettes, canvas, caret, LaTeX/MathML/OMML import);
//! - `--to <format> [--from <format>] [--in file] [--out file] <text>`: one conversion, for
//!   scripts and for host add-ins that can launch a process;
//! - `--serve [port]`: the same conversions over loopback HTTP, for host add-ins that cannot
//!   (WPS loads its add-ins as web pages).

mod serve;

use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Pos2, Rect, Sense, Stroke, Vec2,
};
use mt_edit::{Cursor, Editor, PaletteItem, palettes};
use mt_layout::{layout, layout_with_anchors, to_svg, Box as LayoutBox, LayoutStyle, RowAnchor};
use mt_render::metrics::{FaceMetrics, MATH_FONT};

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

/// Lay the equation out and produce one of the image formats.
fn render_image(math: &mt_core::Math, to: Format, size: f64, scale: f64) -> Result<Vec<u8>, String> {
    let metrics = FaceMetrics::parse(MATH_FONT, 0).ok_or("bundled math font could not be parsed")?;
    let style = LayoutStyle::display(size);
    let boxes = layout(math, &metrics, &style);
    match to {
        Format::Svg => Ok(to_svg(&boxes, &style).into_bytes()),
        Format::Png => {
            let options = mt_render::RenderOptions { scale, ..Default::default() };
            // The bounds decide the allocation inside `render_png`, so measure them first and
            // refuse the ones that would exhaust the address space.
            let (width, height) = mt_render::image_size(&boxes, &style, metrics.face(), &options);
            check_render_pixels(width, height)?;
            mt_render::render_png(&boxes, &style, metrics.face(), &options)
                .map_err(|error| format!("cannot rasterise: {error}"))
        }
        Format::Eps => Ok(mt_render::render_eps(&boxes, &style, metrics.face(), &mt_render::RenderOptions::default())
            .into_bytes()),
        _ => Err(format!("{} is not an image format", to.name())),
    }
}

/// Write an equation out in the requested format.
fn write_math(math: &mt_core::Math, to: Format, size: f64, scale: f64) -> Result<Vec<u8>, String> {
    Ok(match to {
        Format::Latex => mt_tex::to_latex(math).into_bytes(),
        Format::MathMl => mt_mathml::write(math).into_bytes(),
        Format::Omml => mt_omml::write(math).into_bytes(),
        Format::Mtef => mt_mtef::write(math),
        Format::Svg | Format::Png | Format::Eps => render_image(math, to, size, scale)?,
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
    tex_input: String,
    status: String,
    /// File the equation is read from and written back to, when opened from a document object.
    path: Option<std::path::PathBuf>,
    dirty: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        let metrics = FaceMetrics::parse(MATH_FONT, 0).expect("bundled math font must parse");
        Self {
            editor: Editor::new(),
            metrics,
            style: LayoutStyle::display(18.0),
            tex_input: String::from(r"\frac{a}{b} + \sqrt{x_i^2}"),
            status: "输入字符或点选面板插入模板；左右方向键在槽位间移动".to_owned(),
            path: None,
            dirty: false,
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

    fn apply_palette_item(&mut self, item: PaletteItem) {
        self.dirty = true;
        match item {
            PaletteItem::Symbol { ch, class, font } => self.editor.insert_symbol(ch, class, font),
            PaletteItem::Template(template) => self.editor.insert_template(template),
            PaletteItem::Space(space) => self.editor.insert_space(space),
        }
    }

    /// Import whatever the input box holds, picking the format from the text itself.
    fn import_text(&mut self) {
        let text = self.tex_input.clone();
        let from = detect_format(&text);
        match read_math(text.as_bytes(), from) {
            Ok(math) => {
                self.editor.set_math(math);
                self.dirty = true;
                self.status = format!("已导入（按 {} 解析）", from.name());
            }
            Err(message) => self.status = format!("导入失败：{message}"),
        }
    }

    /// Put the equation on the clipboard in one of the interchange formats.
    fn copy_as(&mut self, ctx: &egui::Context, format: Format) {
        match write_math(self.editor.math(), format, self.style.size, 3.0) {
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
        let style = LayoutStyle::display(self.style.size.min(16.0));
        let boxes = layout(&math, &self.metrics, &style);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), Sense::hover());
        let painter = ui.painter_at(rect);
        let origin = Pos2::new(rect.left() + 8.0, rect.center().y);
        let font_id = |size: f64| FontId::new(size as f32, FontFamily::Name(MATH_FAMILY.into()));
        draw_boxes(&painter, &boxes, origin, &self.metrics, ui.visuals().weak_text_color(), &font_id);
    }

    /// Write the equation to a file in the requested image format.
    fn export(&mut self, format: Format) {
        match write_math(self.editor.math(), format, self.style.size, 3.0) {
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
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
                ui.add(
                    egui::Slider::new(&mut self.style.size, 8.0..=48.0)
                        .text("字号")
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
                if self.path.is_some() {
                    let label = if self.dirty { "保存 *" } else { "保存" };
                    if ui.button(label).clicked() {
                        self.save();
                    }
                }
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            // Bottom-up: the footer claims its natural height first, the canvas then takes whatever
            // is left, so neither depends on a hand-computed height.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                self.draw_preview(ui);
                ui.label(&self.status);
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
                let origin = Pos2::new(rect.left() + 24.0, rect.center().y);
                let font_id = |size: f64| FontId::new(size as f32, FontFamily::Name(MATH_FAMILY.into()));
                draw_boxes(&painter, &boxes, origin, &self.metrics, color, &font_id);
                self.draw_caret(&painter, origin, &anchors, color);

                if let Some(point) = response.interact_pointer_pos() {
                    if let Some(target) = caret_at(&anchors, origin, point) {
                        if response.dragged() {
                            self.editor.extend_to(target);
                        } else if response.clicked() {
                            self.editor.set_cursor(target);
                        }
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

/// Nearest insertion position to `point`, weighted so that staying on the caret's own line wins
/// over a slightly smaller horizontal distance on a line further away.
fn caret_at(anchors: &[RowAnchor], origin: Pos2, point: Pos2) -> Option<Cursor> {
    let mut best: Option<(f32, Cursor)> = None;
    for anchor in anchors {
        let baseline = origin.y + anchor.origin.1 as f32;
        for (index, offset) in anchor.offsets.iter().enumerate() {
            let x = origin.x + (anchor.origin.0 + offset) as f32;
            let cost = (point.x - x).abs() + (point.y - baseline).abs() * 8.0;
            if best.as_ref().is_none_or(|(best_cost, _)| cost < *best_cost) {
                best = Some((cost, Cursor { path: anchor.path.clone(), index }));
            }
        }
    }
    best.map(|(_, cursor)| cursor)
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
}
