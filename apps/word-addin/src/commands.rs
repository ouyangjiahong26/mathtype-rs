//! The commands the ribbon buttons run.
//!
//! Each command reads the selection, does the conversion with the equation crates, and writes the
//! result back into the selection, returning the message the status bar shows.

use std::path::{Path, PathBuf};

use mt_layout::{ApproxMetrics, LayoutStyle, layout};
use mt_render::{RenderOptions, render_png};
use windows::Win32::Foundation::{DISP_E_MEMBERNOTFOUND, DISP_E_UNKNOWNNAME};
use windows::Win32::System::Com::CLSIDFromProgID;
use windows::core::PCWSTR;

use crate::clipboard;
use crate::com::{Object, Variant};
use crate::word;

/// The Latin Modern Math face every rendering uses.
const FACE: &[u8] = include_bytes!("../../../assets/fonts/latinmodern-math.otf");

/// The size used when the host reports no usable font size for the selection.
const DEFAULT_SIZE: f64 = 11.0;

/// The smallest font size, in points, the host is believed about.
const MIN_SIZE: f64 = 4.0;

/// The largest font size, in points, the host is believed about.
const MAX_SIZE: f64 = 200.0;

/// The font size to render at: the host's value when it is a usable size, the default otherwise.
///
/// `Selection.Font.Size` is `wdUndefined` (9999999) when the selection mixes sizes, and that is not
/// a size: handing it to the layout engine asks it to lay an equation out that many points tall
/// and, at that magnitude, to allocate until the process aborts. Anything outside the range a font
/// size can have — including `NaN`, which compares false against every bound — falls back to the
/// default instead.
fn render_size(size: Option<f64>) -> f64 {
    size.filter(|size| size.is_finite() && (MIN_SIZE..=MAX_SIZE).contains(size)).unwrap_or(DEFAULT_SIZE)
}

/// What a command reports: the message for the status bar, or why it could not run.
pub type Outcome = Result<String, String>;

/// Names the step that failed, for the message the user is shown.
fn failed(operation: &'static str) -> impl Fn(windows::core::Error) -> String {
    move |error| format!("{operation}: {error}")
}

/// Turns the selected LaTeX into a native Word equation.
pub fn to_equation(application: &Object) -> Outcome {
    let selection = word::selection(application).map_err(failed("the selection could not be read"))?;
    if equations_in(&selection)? > 0 {
        return Err("the selection already holds an equation".to_string());
    }
    let latex = selected_latex(&selection)?;
    let math = mt_tex::parse(&latex).map_err(|error| format!("not LaTeX: {error}"))?;
    let markup = mt_omml::write(&math);
    clipboard::put_equation(&markup, &latex).map_err(failed("the clipboard could not be filled"))?;
    selection.call("Paste", Vec::new()).map_err(failed("the equation could not be pasted"))?;
    Ok(format!("equation inserted: {latex}"))
}

/// Turns the selected equation back into LaTeX text.
pub fn to_latex(application: &Object) -> Outcome {
    let selection = word::selection(application).map_err(failed("the selection could not be read"))?;
    let equation = word::selected_equation(&selection)
        .map_err(failed("the selection could not be read"))?
        .ok_or_else(|| "the selection does not hold an equation".to_string())?;
    let range = equation.get_object("Range").map_err(failed("the equation could not be read"))?;
    let xml = range.get_string("WordOpenXML").map_err(failed("the equation could not be read"))?;
    let markup = word::extract_math(&xml).ok_or_else(|| "the equation holds no Office Math markup".to_string())?;
    let math = mt_omml::parse(markup).map_err(|error| format!("unreadable equation: {error}"))?;
    let latex = mt_tex::to_latex(&math);
    word::replace_equation(&equation, &latex).map_err(failed("the equation could not be replaced"))?;
    Ok(format!("converted to LaTeX: {latex}"))
}

/// Converts whichever way fits the selection.
pub fn toggle(application: &Object) -> Outcome {
    let selection = word::selection(application).map_err(failed("the selection could not be read"))?;
    if word::selected_equation(&selection).map_err(failed("the selection could not be read"))?.is_some() {
        to_latex(application)
    } else {
        to_equation(application)
    }
}

/// Renders the selected LaTeX to a PNG next to the document.
pub fn export_png(application: &Object) -> Outcome {
    let selection = word::selection(application).map_err(failed("the selection could not be read"))?;
    let latex = selected_latex(&selection)?;
    let math = mt_tex::parse(&latex).map_err(|error| format!("not LaTeX: {error}"))?;
    let document = word::active_document(application).map_err(failed("the document could not be read"))?;
    let directory = document.get_string("Path").map_err(failed("the document could not be read"))?;
    let name = document.get_string("Name").map_err(failed("the document could not be read"))?;
    let size = render_size(
        selection.get_object("Font").and_then(|font| font.get("Size")).and_then(|size| size.number()).ok(),
    );

    let face = ttf_parser::Face::parse(FACE, 0).map_err(|error| format!("the bundled font could not be read: {error}"))?;
    let style = LayoutStyle::display(size);
    let tree = layout(&math, &ApproxMetrics, &style);
    let image = render_png(&tree, &style, &face, &RenderOptions::default())
        .map_err(|error| format!("the image could not be encoded: {error}"))?;
    let target = image_path(&directory, &name);
    std::fs::write(&target, &image).map_err(|error| format!("{} could not be written: {error}", target.display()))?;
    Ok(format!("image written to {}", target.display()))
}

/// Exports the whole document as a web page directory next to it: `index.html` plus `media/`,
/// formulas as inline MathML.
pub fn export_html(application: &Object) -> Outcome {
    let document = word::active_document(application).map_err(failed("the document could not be read"))?;
    let directory = document.get_string("Path").map_err(failed("the document could not be read"))?;
    let name = document.get_string("Name").map_err(failed("the document could not be read"))?;
    let path = document.get_string("FullName").map_err(failed("the document could not be read"))?;
    if path.is_empty() || directory.is_empty() {
        return Err("the document has never been saved".to_owned());
    }
    let bytes = std::fs::read(&path).map_err(|error| format!("{path} could not be read: {error}"))?;
    let opened = mt_docx::Document::open(&bytes)
        .map_err(|error| format!("{path} could not be opened: {error}"))?;
    let stem = Path::new(&name).file_stem().map_or_else(
        || "document".to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let target = PathBuf::from(&directory).join(format!("{stem}-web"));
    let page = mt_html::export_files(&opened, &mt_html::Options::default(), &target)
        .map_err(|error| format!("the web page could not be written: {error}"))?;
    Ok(format!("web page written to {}", page.display()))
}

/// Batch-converts every formula of the whole document (or of the selection, when there is one)
/// to Office Math (#13).
///
/// The document is read from disk (`FullName`) after a save, converted by the shared
/// `mt-batch` pipeline, and the result is **saved next to** the original as
/// `<名>-converted.docx`, never in place — the host keeps the original open. A selection that is
/// neither collapsed nor the whole document is marked with a temporary bookmark
/// (`MTRSBatchScope`), whose byte range in `word/document.xml` becomes the pipeline's scope; the
/// bookmark itself stays in the output and can be deleted there. When no bookmark can be placed
/// or found, the whole document is converted.
pub fn batch_convert(application: &Object) -> Outcome {
    let document = word::active_document(application).map_err(failed("the document could not be read"))?;
    let directory = document.get_string("Path").map_err(failed("the document could not be read"))?;
    let name = document.get_string("Name").map_err(failed("the document could not be read"))?;
    let full_name = document.get_string("FullName").map_err(failed("the document could not be read"))?;
    if full_name.is_empty() || directory.is_empty() {
        return Err("the document has never been saved".to_owned());
    }

    // Selection scope, best effort: a real sub-selection is bookmarked so the pipeline can cut
    // its range out of the document's markup. Any failure here falls back to the whole document.
    let mut scoped = false;
    if let Some(selection) = word::selection(application).ok() {
        let bounds = (selection.get_i32("Start"), selection.get_i32("End"));
        let content = document.get_object("Content").map_err(failed("the document could not be read"))?;
        let whole = (content.get_i32("Start"), content.get_i32("End"));
        if let (Ok(start), Ok(end), Ok(content_start), Ok(content_end)) =
            (bounds.0, bounds.1, whole.0, whole.1)
        {
            let partial = start != end && !(start == content_start && end == content_end);
            if partial {
                if selection
                    .get_object("Bookmarks")
                    .and_then(|bookmarks| bookmarks.call_object("Add", vec![Variant::bstr(SCOPE_BOOKMARK)]))
                    .is_ok()
                {
                    scoped = true;
                }
            }
        }
    }
    // The bookmark only exists on disk once the document is saved; saving is what a batch run
    // wants anyway. `Saved` being false only means the disk copy is stale.
    let saved = document.get("Saved").and_then(|value| value.number()).map(|value| value != 0.0).unwrap_or(false);
    if !saved {
        document.call("Save", Vec::new()).map_err(failed("the document could not be saved"))?;
    }

    let bytes = std::fs::read(&full_name).map_err(|error| format!("{full_name} could not be read: {error}"))?;
    let scope = scope_of(&bytes, scoped);
    let directory = PathBuf::from(&directory);
    let options = mt_batch::Options {
        format: mt_batch::TargetFormat::OMath,
        mode: mt_batch::Mode::Replace,
        dpi: mt_batch::DEFAULT_DPI,
        size: mt_batch::DEFAULT_SIZE,
        export_dir: directory.clone(),
        scope,
    };
    let outcome = mt_batch::run(&bytes, &options, &mut |_| true).map_err(|error| format!("批量转换失败: {error}"))?;
    let report = &outcome.report;

    let stem = Path::new(&name).file_stem().map_or_else(
        || "document".to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let target = directory.join(format!("{stem}-converted.docx"));
    if let Some(docx) = &outcome.docx {
        std::fs::write(&target, docx).map_err(|error| format!("{} could not be written: {error}", target.display()))?;
    }
    let report_path = directory.join(format!("{stem}-batch-report.txt"));
    std::fs::write(&report_path, report.to_text())
        .map_err(|error| format!("{} could not be written: {error}", report_path.display()))?;

    let scope_note = if scoped { "（仅选区内的公式）" } else { "（整篇文档）" };
    Ok(format!(
        "批量转换{}：共 {} 个，成功 {}，失败 {}；另存为 {}，报告 {}",
        scope_note,
        report.total,
        report.succeeded,
        report.failures.len(),
        target.display(),
        report_path.display()
    ))
}

/// The name of the bookmark that marks a selection's scope.
const SCOPE_BOOKMARK: &str = "MTRSBatchScope";

/// The byte range of the scope bookmark in `word/document.xml`, when it is there and whole.
///
/// The pipeline works on the file's markup, and a bookmark is the one marker that survives the
/// round trip through the host's save. When anything about it cannot be found, `None` means the
/// whole document.
fn scope_of(bytes: &[u8], scoped: bool) -> Option<(usize, usize)> {
    if !scoped {
        return None;
    }
    let document = mt_docx::Document::open(bytes).ok()?;
    let xml = document.document_xml();
    let name = xml.find(format!("w:name=\"{SCOPE_BOOKMARK}\"").as_str())?;
    let start = xml[..name].rfind("<w:bookmarkStart")?;
    let attributes = &xml[start..name];
    let id = attributes.split("w:id=\"").nth(1)?.split('"').next()?.to_owned();
    let end_marker = format!("w:id=\"{id}\"");
    let end = xml[name..].find("<w:bookmarkEnd")? + name;
    if xml[end..].contains(&end_marker) {
        Some((start, end))
    } else {
        None
    }
}

/// The class of the object server that provides the embedded equation.
const OBJECT_SERVER: &str = "MathTypeRs.Equation.1";

/// Whether the object server is registered at all.
///
/// The class id is looked up the way any client would, so a server installed for another user or
/// not installed at all is reported before the host is asked to create the object.
fn object_server_registered() -> bool {
    let class: Vec<u16> = OBJECT_SERVER.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: the name is NUL terminated.
    unsafe { CLSIDFromProgID(PCWSTR(class.as_ptr())) }.is_ok()
}

/// What to say when the object server is not there.
fn missing_object_server() -> String {
    format!("the {OBJECT_SERVER} object server is not registered; run install.ps1 in apps/ole-server")
}

/// Inserts the selected LaTeX as an embedded equation object.
///
/// The document then holds a real OLE object rather than a picture of an equation: double-clicking
/// it opens the editor that owns it, and saving there writes the edited equation back into the
/// object, and so into the document.
pub fn insert_object(application: &Object) -> Outcome {
    // A host asked for an object class that is not registered may show a dialog and wait, so the
    // class is looked up before the host is asked to create anything.
    if !object_server_registered() {
        return Err(missing_object_server());
    }
    let selection = word::selection(application).map_err(failed("the selection could not be read"))?;
    let latex = selected_latex(&selection)?;
    // The object is created before anything can be handed to it, so the source is left on the
    // clipboard for its server to pick up as it starts; it is given the source properly below. It
    // is also what puts the source back when a step below fails, so it is filled before the source
    // can be cleared.
    clipboard::put_latex(&latex).map_err(failed("the clipboard could not be filled"))?;
    // The object takes the place of the source text.
    selection.call("Delete", Vec::new()).map_err(failed("the selection could not be cleared"))?;
    let shapes = selection.get_object("InlineShapes").map_err(failed("the selection could not be reached"))?;
    let shape = match shapes.call_object(
        "AddOLEObject",
        vec![Variant::bstr(OBJECT_SERVER), Variant::missing(), Variant::boolean(false), Variant::boolean(false)],
    ) {
        Ok(shape) => shape,
        Err(error) => {
            // The source was cleared before the object could be created, so it has to be put back
            // before the error is reported.
            return Err(format!(
                "the {OBJECT_SERVER} object server could not be started: {error}; run install.ps1 in apps/ole-server{}",
                restore_source(&selection)
            ));
        }
    };
    // Only an object that took and stored the equation counts as inserted; otherwise the object is
    // removed and the source put back, so the document is not left with an empty object in place of
    // the equation.
    if let Err(error) = hand_over(&shape, &latex) {
        let _ = shape.call("Delete", Vec::new());
        return Err(format!("{error}{}", restore_source(&selection)));
    }
    Ok(format!("equation object inserted: {latex}"))
}

/// Puts the source text back from the clipboard after a failure, and says what happened.
///
/// The LaTeX was left on the clipboard before the source was cleared, so pasting it restores the
/// user's text. The message records whether that worked, because a failure that could not be undone
/// has to be told apart from one that was.
fn restore_source(selection: &Object) -> String {
    match selection.call("Paste", Vec::new()) {
        Ok(_) => "; the source text was put back".to_string(),
        Err(error) => format!("; the source text could not be put back and is still on the clipboard ({error})"),
    }
}

/// Hands the source to the object server and asks it to store it.
///
/// Until this happens the object holds no equation, so it is part of inserting one rather than a
/// later step. Both steps have to succeed and both members have to exist: an object that cannot
/// take the source, or has nowhere to store it, is not handed the source at all, so a failure here
/// is told apart from a success instead of being reported as one.
fn hand_over(shape: &Object, latex: &str) -> Result<(), String> {
    let format = shape.get_object("OLEFormat").map_err(failed("the equation object could not be reached"))?;
    let server = format.get_object("Object").map_err(failed("the equation object has no server"))?;
    match server.call("SetLatex", vec![Variant::bstr(latex)]) {
        Ok(_) => {}
        Err(error) if absent_member(&error) => {
            return Err("the equation object does not accept LaTeX (no SetLatex member)".to_string())
        }
        Err(error) => return Err(failed("the equation object did not take the LaTeX")(error)),
    }
    match server.call("Save", Vec::new()) {
        Ok(_) => Ok(()),
        Err(error) if absent_member(&error) => {
            Err("the equation object has nowhere to store the equation (no Save member)".to_string())
        }
        Err(error) => Err(failed("the equation object could not be stored")(error)),
    }
}

/// Whether an error says that the member is not there at all.
fn absent_member(error: &windows::core::Error) -> bool {
    error.code() == DISP_E_MEMBERNOTFOUND || error.code() == DISP_E_UNKNOWNNAME
}

/// A command the ribbon can invoke.
///
/// The name is the member Office finds with `GetIDsOfNames`; the id is what the object answers to
/// through `Invoke`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// `InsertEquationObject`: the selected LaTeX becomes an embedded equation object.
    InsertObject,
    /// `ConvertSelectionToEquation`: the selected LaTeX becomes a Word equation.
    ToEquation,
    /// `ConvertSelectionToLatex`: the selected equation becomes LaTeX text.
    ToLatex,
    /// `TeXToggle`: whichever of the two fits the selection.
    Toggle,
    /// `ExportSelectionPng`: the selected LaTeX is rendered next to the document.
    ExportPng,
    /// `ExportDocumentHtml`: the document becomes a web page directory next to itself.
    ExportHtml,
    /// `BatchConvertDocument`: every formula of the document (or selection) becomes Office Math.
    BatchConvert,
}

impl Command {
    /// Every command, for the name and member id lookups.
    const ALL: [Self; 7] = [
        Self::InsertObject,
        Self::ToEquation,
        Self::ToLatex,
        Self::Toggle,
        Self::ExportPng,
        Self::ExportHtml,
        Self::BatchConvert,
    ];

    /// The member name Office finds the command under.
    pub const fn name(self) -> &'static str {
        match self {
            Self::InsertObject => "InsertEquationObject",
            Self::ToEquation => "ConvertSelectionToEquation",
            Self::ToLatex => "ConvertSelectionToLatex",
            Self::Toggle => "TeXToggle",
            Self::ExportPng => "ExportSelectionPng",
            Self::ExportHtml => "ExportDocumentHtml",
            Self::BatchConvert => "BatchConvertDocument",
        }
    }

    /// The member id the command answers to.
    pub const fn dispid(self) -> i32 {
        match self {
            Self::InsertObject => 100,
            Self::ToEquation => 101,
            Self::ToLatex => 102,
            Self::Toggle => 103,
            Self::ExportPng => 104,
            Self::ExportHtml => 105,
            Self::BatchConvert => 106,
        }
    }

    /// The command a member name names, if it names one.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.name().eq_ignore_ascii_case(name))
    }

    /// The command a member id names, if it names one.
    pub fn from_dispid(dispid: i32) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.dispid() == dispid)
    }

    /// Runs the command against the host application.
    pub fn run(self, application: &Object) -> Outcome {
        match self {
            Self::InsertObject => insert_object(application),
            Self::ToEquation => to_equation(application),
            Self::ToLatex => to_latex(application),
            Self::Toggle => toggle(application),
            Self::ExportPng => export_png(application),
            Self::ExportHtml => export_html(application),
            Self::BatchConvert => batch_convert(application),
        }
    }
}

/// How many equations an object holds.
fn equations_in(scope: &Object) -> Result<i32, String> {
    let equations = word::equations(scope).map_err(failed("the selection could not be read"))?;
    word::count(&equations).map_err(failed("the selection could not be read"))
}

/// The text of the selection, without the paragraph marks `Range.Text` adds at its end.
fn selected_latex(selection: &Object) -> Result<String, String> {
    let text = word::text(selection).map_err(failed("the selection could not be read"))?;
    let latex = text.trim();
    if latex.is_empty() {
        return Err("nothing is selected".to_string());
    }
    Ok(latex.to_owned())
}

/// Where a rendered image goes: next to the document, or in the temporary directory when the
/// document has never been saved.
fn image_path(directory: &str, name: &str) -> PathBuf {
    let stem = Path::new(name).file_stem().map_or_else(|| "document".to_string(), |stem| stem.to_string_lossy().into_owned());
    let directory = if directory.is_empty() { std::env::temp_dir() } else { PathBuf::from(directory) };
    directory.join(format!("{stem}-equation.png"))
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_SIZE, image_path, render_size};

    #[test]
    fn the_image_sits_next_to_the_document() {
        let path = image_path(r"C:\docs", "paper.docx");
        assert_eq!(path, std::path::Path::new(r"C:\docs").join("paper-equation.png"));
    }

    #[test]
    fn an_unsaved_document_falls_back_to_the_temporary_directory() {
        let path = image_path("", "Document1");
        assert!(path.starts_with(std::env::temp_dir()));
        assert_eq!(path.file_name().unwrap(), "Document1-equation.png");
    }

    #[test]
    fn an_undefined_font_size_falls_back_to_the_default() {
        // `wdUndefined` is what Word reports for `Font.Size` when the selection mixes sizes.
        assert_eq!(render_size(Some(9_999_999.0)), DEFAULT_SIZE);
    }

    #[test]
    fn a_nonsensical_font_size_falls_back_to_the_default() {
        assert_eq!(render_size(Some(f64::NAN)), DEFAULT_SIZE);
        assert_eq!(render_size(Some(f64::INFINITY)), DEFAULT_SIZE);
        assert_eq!(render_size(Some(-1.0)), DEFAULT_SIZE);
        assert_eq!(render_size(Some(0.0)), DEFAULT_SIZE);
        assert_eq!(render_size(None), DEFAULT_SIZE);
    }

    #[test]
    fn a_size_within_the_usable_range_is_kept() {
        assert_eq!(render_size(Some(4.0)), 4.0);
        assert_eq!(render_size(Some(11.0)), 11.0);
        assert_eq!(render_size(Some(200.0)), 200.0);
    }

    #[test]
    fn a_size_outside_the_usable_range_falls_back_to_the_default() {
        assert_eq!(render_size(Some(3.9)), DEFAULT_SIZE);
        assert_eq!(render_size(Some(201.0)), DEFAULT_SIZE);
    }
}
