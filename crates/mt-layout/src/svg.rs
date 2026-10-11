//! SVG preview of a laid-out box.
//!
//! The output is a self-contained document: one `<text>` per glyph and one `<rect>` per rule, with
//! no external references, so it can be shown in a browser, embedded in a report or diffed in a
//! test.

use crate::constants::{PLACEHOLDER_DASH, PLACEHOLDER_STROKE, SVG_PADDING};
use crate::{Box, LayoutStyle};

/// Font stack every glyph is set in.
const FONT_FAMILY: &str = "Latin Modern Math, STIX Two Math, Cambria Math, serif";

/// Stroke colour of the dashed empty-slot placeholder.
const PLACEHOLDER_COLOR: &str = "#888888";

/// The rectangle `to_svg` draws into, as `x`, `y`, `width` and `height`.
///
/// `y` is negative because the root box's baseline sits `height` points below its top edge.
pub fn view_box(root: &Box, style: &LayoutStyle) -> (f64, f64, f64, f64) {
    let padding = SVG_PADDING * style.size;
    let width = root.width() + 2.0 * padding;
    let height = root.height() + root.depth() + 2.0 * padding;
    (-padding, -root.height() - padding, width, height)
}

/// Render `root` as a self-contained SVG document.
pub fn to_svg(root: &Box, style: &LayoutStyle) -> String {
    let (x, y, width, height) = view_box(root, style);
    let mut svg = String::new();
    svg.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\"");
    svg.push_str(&format!(" viewBox=\"{} {} {} {}\"", number(x), number(y), number(width), number(height)));
    svg.push_str(&format!(" width=\"{}pt\" height=\"{}pt\">\n", number(width), number(height)));
    write_box(&mut svg, root);
    svg.push_str("</svg>\n");
    svg
}

fn write_box(svg: &mut String, node: &Box) {
    match node {
        Box::HBox { items, .. } | Box::VBox { items, .. } => {
            for item in items {
                write_box(svg, item);
            }
        }
        Box::Kern { .. } => {}
        Box::Glyph { ch, font, x, y, size, yscale } => {
            let (x, y, size) = (number(*x), number(*y), number(*size));
            svg.push_str(&format!("<text x=\"{x}\" y=\"{y}\" font-size=\"{size}\""));
            svg.push_str(&format!(" font-family=\"{FONT_FAMILY}\""));
            if font.is_italic() {
                svg.push_str(" font-style=\"italic\"");
            }
            if font.is_bold() {
                svg.push_str(" font-weight=\"bold\"");
            }
            if *yscale != 1.0 {
                let stretched = number(*yscale);
                svg.push_str(&format!(
                    " transform=\"translate({x} {y}) scale(1 {stretched}) translate(-{x} -{y})\""
                ));
            }
            svg.push('>');
            push_escaped(svg, *ch);
            svg.push_str("</text>\n");
        }
        Box::Rule { x, y, width, height, depth, dashed } => {
            let top = number(y - height);
            let (x, width, full_height) = (number(*x), number(*width), number(height + depth));
            if *dashed {
                let dash = number(PLACEHOLDER_DASH);
                svg.push_str(&format!(
                    "<rect x=\"{x}\" y=\"{top}\" width=\"{width}\" height=\"{full_height}\" fill=\"none\" stroke=\"{PLACEHOLDER_COLOR}\" stroke-width=\"{}\" stroke-dasharray=\"{dash} {dash}\"/>\n",
                    number(PLACEHOLDER_STROKE)
                ));
            } else {
                svg.push_str(&format!(
                    "<rect x=\"{x}\" y=\"{top}\" width=\"{width}\" height=\"{full_height}\" fill=\"black\"/>\n"
                ));
            }
        }
    }
}

/// Format a coordinate: three decimals, trailing zeros removed, never `-0`.
fn number(value: f64) -> String {
    let rounded = if value == 0.0 { 0.0 } else { (value * 1000.0).round() / 1000.0 };
    format!("{rounded}")
}

/// Append `ch` with the XML special characters escaped.
fn push_escaped(svg: &mut String, ch: char) {
    match ch {
        '&' => svg.push_str("&amp;"),
        '<' => svg.push_str("&lt;"),
        '>' => svg.push_str("&gt;"),
        '"' => svg.push_str("&quot;"),
        '\'' => svg.push_str("&apos;"),
        other => svg.push(other),
    }
}
