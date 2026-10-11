//! The markup this crate writes into a document: replacement items and inserted fragments.
//!
//! Every fragment is namespace-complete on its own root, so it can be spliced into any
//! `word/document.xml` no matter which prefixes the document happens to bind.

use mt_core::Math;

use crate::content_types::escape_into;
use crate::model::{RunStyle, VertAlign};

/// The WordprocessingML namespace.
pub(crate) const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
/// The office-document relationship namespace, the one `r:id` lives in.
pub(crate) const R: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
/// The Microsoft Office namespace `o:OLEObject` lives in.
pub(crate) const O: &str = "urn:schemas-microsoft-com:office:office";
/// The WordprocessingML drawing placement namespace.
pub(crate) const WP: &str =
    "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
/// The DrawingML main namespace.
pub(crate) const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
/// The DrawingML picture namespace.
pub(crate) const PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";

/// A run of text, optionally styled, replacing or following an item.
pub(crate) fn text_run(text: &str, style: &RunStyle) -> String {
    let mut out = String::from("<w:r xmlns:w=\"");
    out.push_str(W);
    out.push_str("\">");
    run_properties(&mut out, style);
    out.push_str("<w:t xml:space=\"preserve\">");
    escape_into(&mut out, text);
    out.push_str("</w:t></w:r>");
    out
}

/// The `w:rPr` of a generated run, only the parts that are set.
fn run_properties(out: &mut String, style: &RunStyle) {
    if style == &RunStyle::default() {
        return;
    }
    out.push_str("<w:rPr>");
    let toggles = [(style.bold, "b"), (style.italic, "i")];
    for (setting, name) in toggles {
        match setting {
            Some(true) => {
                out.push_str("<w:");
                out.push_str(name);
                out.push_str("/><w:");
                out.push_str(name);
                out.push_str("Cs/>");
            }
            Some(false) => {
                out.push_str("<w:");
                out.push_str(name);
                out.push_str(" w:val=\"0\"/><w:");
                out.push_str(name);
                out.push_str("Cs w:val=\"0\"/>");
            }
            None => {}
        }
    }
    if let Some(size) = style.size_half_points {
        out.push_str("<w:sz w:val=\"");
        out.push_str(&size.to_string());
        out.push_str("\"/><w:szCs w:val=\"");
        out.push_str(&size.to_string());
        out.push_str("\"/>");
    }
    if let Some(alignment) = style.vert_align {
        out.push_str("<w:vertAlign w:val=\"");
        out.push_str(match alignment {
            VertAlign::Superscript => "superscript",
            VertAlign::Subscript => "subscript",
        });
        out.push_str("\"/>");
    }
    out.push_str("</w:rPr>");
}

/// An Office Math formula, inline or displayed, straight from `mt-omml`.
pub(crate) fn o_math(math: &Math, display: bool) -> String {
    if display {
        mt_omml::write_paragraph(math, None)
    } else {
        mt_omml::write(math)
    }
}

/// An inline picture. `extent` is the size in English Metric Units (914400 per inch); `doc_pr_id`
/// must be unique among the drawings of the document.
pub(crate) fn image_run(
    rel_id: &str,
    name: &str,
    extent: (u32, u32),
    doc_pr_id: u32,
) -> String {
    let (cx, cy) = extent;
    let mut out = String::from("<w:r xmlns:w=\"");
    out.push_str(W);
    out.push_str("\" xmlns:r=\"");
    out.push_str(R);
    out.push_str("\"><w:drawing><wp:inline xmlns:wp=\"");
    out.push_str(WP);
    out.push_str("\" distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">");
    out.push_str("<wp:extent cx=\"");
    out.push_str(&cx.to_string());
    out.push_str("\" cy=\"");
    out.push_str(&cy.to_string());
    out.push_str("\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/>");
    out.push_str("<wp:docPr id=\"");
    out.push_str(&doc_pr_id.to_string());
    out.push_str("\" name=\"");
    escape_into(&mut out, name);
    out.push_str("\"/><wp:cNvGraphicFramePr><a:graphicFrameLocks xmlns:a=\"");
    out.push_str(A);
    out.push_str("\" noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>");
    out.push_str("<a:graphic xmlns:a=\"");
    out.push_str(A);
    out.push_str("\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">");
    out.push_str("<pic:pic xmlns:pic=\"");
    out.push_str(PIC);
    out.push_str("\"><pic:nvPicPr><pic:cNvPr id=\"");
    out.push_str(&doc_pr_id.to_string());
    out.push_str("\" name=\"");
    escape_into(&mut out, name);
    out.push_str("\"/><pic:cNvPicPr/></pic:nvPicPr>");
    out.push_str("<pic:blipFill><a:blip r:embed=\"");
    escape_into(&mut out, rel_id);
    out.push_str("\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>");
    out.push_str("<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"");
    out.push_str(&cx.to_string());
    out.push_str("\" cy=\"");
    out.push_str(&cy.to_string());
    out.push_str("\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>");
    out.push_str("</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>");
    out
}

/// An embedded object run: the `w:object` markup with its OLE reference, and the original
/// preview markup put back when there is some.
pub(crate) fn object_run(
    data_rel_id: &str,
    prog_id: &str,
    preview: Option<&str>,
    shape_id: u32,
) -> String {
    let mut out = String::from("<w:r xmlns:w=\"");
    out.push_str(W);
    out.push_str("\"><w:object>");
    if let Some(preview) = preview {
        out.push_str(preview);
    }
    out.push_str("<o:OLEObject xmlns:o=\"");
    out.push_str(O);
    out.push_str("\" xmlns:r=\"");
    out.push_str(R);
    out.push_str("\" Type=\"Embed\" ProgID=\"");
    escape_into(&mut out, prog_id);
    out.push_str("\" ShapeID=\"_mt_shape_");
    out.push_str(&shape_id.to_string());
    out.push_str("\" DrawAspect=\"Content\" ObjectID=\"_mt_object_");
    out.push_str(&shape_id.to_string());
    out.push_str("\" r:id=\"");
    escape_into(&mut out, data_rel_id);
    out.push_str("\"/></w:object></w:r>");
    out
}

/// A whole paragraph holding nothing but the given run-level fragment, for body-level insertion.
pub(crate) fn paragraph(run_xml: &str) -> String {
    let mut out = String::from("<w:p xmlns:w=\"");
    out.push_str(W);
    out.push_str("\">");
    out.push_str(run_xml);
    out.push_str("</w:p>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_runs_carry_their_style() {
        assert_eq!(text_run("a<b", &RunStyle::default()).contains("a&lt;b"), true);
        let styled = RunStyle {
            bold: Some(true),
            italic: None,
            vert_align: Some(VertAlign::Superscript),
            size_half_points: Some(24),
        };
        let xml = text_run("x", &styled);
        assert!(xml.contains("<w:b/>"));
        assert!(xml.contains("<w:sz w:val=\"24\"/>"));
        assert!(xml.contains("superscript"));
    }

    #[test]
    fn object_runs_reference_their_part() {
        let xml = object_run("rId5", "Equation.3", None, 7);
        assert!(xml.contains("ProgID=\"Equation.3\""));
        assert!(xml.contains("r:id=\"rId5\""));
        assert!(xml.contains("ObjectID=\"_mt_object_7\""));
    }
}
