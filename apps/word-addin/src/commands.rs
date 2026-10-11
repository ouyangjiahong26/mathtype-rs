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
    let size = selection
        .get_object("Font")
        .and_then(|font| font.get("Size"))
        .and_then(|size| size.number())
        .unwrap_or(DEFAULT_SIZE);

    let face = ttf_parser::Face::parse(FACE, 0).map_err(|error| format!("the bundled font could not be read: {error}"))?;
    let style = LayoutStyle::display(size);
    let tree = layout(&math, &ApproxMetrics, &style);
    let image = render_png(&tree, &style, &face, &RenderOptions::default())
        .map_err(|error| format!("the image could not be encoded: {error}"))?;
    let target = image_path(&directory, &name);
    std::fs::write(&target, &image).map_err(|error| format!("{} could not be written: {error}", target.display()))?;
    Ok(format!("image written to {}", target.display()))
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
    // clipboard for its server to pick up as it starts; it is given the source properly below.
    clipboard::put_latex(&latex).map_err(failed("the clipboard could not be filled"))?;
    // The object takes the place of the source text.
    selection.call("Delete", Vec::new()).map_err(failed("the selection could not be cleared"))?;
    let shapes = selection.get_object("InlineShapes").map_err(failed("the selection could not be reached"))?;
    let shape = shapes
        .call_object(
            "AddOLEObject",
            vec![Variant::bstr(OBJECT_SERVER), Variant::missing(), Variant::boolean(false), Variant::boolean(false)],
        )
        .map_err(|error| {
            format!(
                "the {OBJECT_SERVER} object server could not be started: {error}; run install.ps1 in apps/ole-server"
            )
        })?;
    hand_over(&shape, &latex)?;
    Ok(format!("equation object inserted: {latex}"))
}

/// Hands the source to the object server and asks it to store it.
///
/// Until this happens the object holds no equation, so it is part of inserting one rather than a
/// later step.
fn hand_over(shape: &Object, latex: &str) -> Result<(), String> {
    let format = shape.get_object("OLEFormat").map_err(failed("the equation object could not be reached"))?;
    let server = format.get_object("Object").map_err(failed("the equation object has no server"))?;
    server.call("SetLatex", vec![Variant::bstr(latex)]).map_err(failed("the equation object did not take the LaTeX"))?;
    match server.call("Save", Vec::new()) {
        Ok(_) => Ok(()),
        // A server whose object has no `Save` has nothing to store into.
        Err(error) if absent_member(&error) => Ok(()),
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
}

impl Command {
    /// Every command, for the name and member id lookups.
    const ALL: [Self; 5] = [Self::InsertObject, Self::ToEquation, Self::ToLatex, Self::Toggle, Self::ExportPng];

    /// The member name Office finds the command under.
    pub const fn name(self) -> &'static str {
        match self {
            Self::InsertObject => "InsertEquationObject",
            Self::ToEquation => "ConvertSelectionToEquation",
            Self::ToLatex => "ConvertSelectionToLatex",
            Self::Toggle => "TeXToggle",
            Self::ExportPng => "ExportSelectionPng",
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
    use super::image_path;

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
}
