//! Export a Word document to a web page.
//!
//! The document index of `mt-docx` is walked once: paragraphs become `p` (headings become `h1`–
//! `h6`), tables are rebuilt from their rows and cells, styled runs map to `strong`/`em`/`sub`/
//! `sup`, and pictures go to a `media/` directory next to the page. Formulas — Office Math and
//! embedded objects alike — are written as inline MathML, so a browser renders them and a copy
//! out of the page is reusable MathML; when the caller asks for it they become PNG pictures
//! instead, and a formula no equation tree could be read from degrades to nothing.
//!
//! Two output shapes share this one generator: a directory with the page plus `media/` (the
//! command line and the Word add-in), and a single self-contained file with every picture as a
//! data URI (the HTTP bridge for the WPS add-in, whose host can only write one file).

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::ops::Range;

use mt_core::Math;
use mt_docx::{Block, Document, Item, Paragraph, RunStyle, Table, VertAlign};
use mt_layout::{Box as LayoutBox, LayoutStyle, layout};
use mt_mathml::write as write_mathml;
use mt_render::metrics::{FaceMetrics, MATH_FONT};
use mt_render::{RenderOptions, render_png};

/// How formulas leave the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathMode {
    /// Inline MathML, rendered by the browser and copyable as MathML.
    MathMl,
    /// PNG pictures, for hosts whose web view cannot render MathML.
    Image,
}

impl MathMode {
    /// The `--math=` / `?math=` spelling of the choice.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "mathml" | "mml" => Some(Self::MathMl),
            "image" | "png" => Some(Self::Image),
            _ => None,
        }
    }
}

/// The optional math rendering script injected into the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    /// No script; a modern browser renders MathML natively.
    None,
    /// MathJax 3, which takes its input as MathML.
    MathJax,
    /// KaTeX with its auto-render extension.
    KaTeX,
}

impl Script {
    /// The `--script=` / `?script=` spelling of the choice.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "mathjax" => Some(Self::MathJax),
            "katex" => Some(Self::KaTeX),
            _ => None,
        }
    }
}

/// What the caller wants written and how.
#[derive(Clone, Debug)]
pub struct Options {
    /// Inline MathML or PNG for the formulas.
    pub math: MathMode,
    /// The optional rendering script injected into the head.
    pub script: Script,
    /// `true` inlines every picture as a data URI, so the page is one file.
    pub embed: bool,
    /// The page's `title`; the document's first heading when `None`.
    pub title: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            math: MathMode::MathMl,
            script: Script::None,
            embed: false,
            title: None,
        }
    }
}

/// One media file the page refers to: its path below the output root, and its bytes.
pub struct MediaFile {
    /// The path as it appears in the HTML, always `/`-separated and below the page.
    pub path: String,
    /// The file's bytes.
    pub bytes: Vec<u8>,
}

/// A finished export: the page and the files it needs.
pub struct Output {
    /// The complete HTML page.
    pub html: String,
    /// The media files, empty when [`Options::embed`] is set.
    pub media: Vec<MediaFile>,
}

/// Exports one document.
pub fn export(doc: &Document, options: &Options) -> Output {
    let mut exporter = Exporter::new(doc, options);
    let body = exporter.body();
    let title = options
        .title
        .clone()
        .or(exporter.inferred_title)
        .unwrap_or_else(|| "文档".to_owned());
    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html lang=\"zh\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<title>");
    html.push_str(&escape(&title));
    html.push_str("</title>\n");
    html.push_str(STYLE);
    html.push_str(script_tag(options.script));
    html.push_str("</head>\n<body>\n");
    html.push_str(&body);
    html.push_str("</body>\n</html>\n");
    Output {
        html,
        media: exporter.media,
    }
}

/// Exports one document as a directory: `index.html` plus `media/` below `directory`, which is
/// created when it does not exist. Returns the path of the page.
pub fn export_files(
    doc: &Document,
    options: &Options,
    directory: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let output = export(doc, options);
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    for file in &output.media {
        let path = directory.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        std::fs::write(&path, &file.bytes)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }
    let page = directory.join("index.html");
    std::fs::write(&page, &output.html)
        .map_err(|error| format!("cannot write {}: {error}", page.display()))?;
    Ok(page)
}

// ------------------------------------------------------------------ the walk

/// Everything one export accumulates: the media list, the equation trees, and the
/// deduplication of pictures shared by several items.
struct Exporter<'a> {
    doc: &'a Document,
    options: &'a Options,
    /// The equation trees of the document, by block and item index.
    equations: HashMap<(usize, usize), Option<Math>>,
    /// Media written so far: source (document part name) to media path, so a picture shared by
    /// two items becomes one file.
    images: HashMap<String, String>,
    /// Media files in write order.
    media: Vec<MediaFile>,
    /// Rendered equation pictures so far, for `media/equationN.png` numbering.
    equation_count: usize,
    /// The first heading's text, the page title when the caller gives none.
    inferred_title: Option<String>,
}

impl<'a> Exporter<'a> {
    fn new(doc: &'a Document, options: &'a Options) -> Self {
        let equations = doc
            .equations()
            .into_iter()
            .map(|equation| ((equation.block, equation.item), equation.math))
            .collect();
        Self {
            doc,
            options,
            equations,
            images: HashMap::new(),
            media: Vec::new(),
            equation_count: 0,
            inferred_title: None,
        }
    }

    /// The body: every top-level block. Blocks inside a table are reached through the table
    /// that holds them, so they are skipped here.
    fn body(&mut self) -> String {
        let mut out = String::new();
        let mut table_range: Option<Range<usize>> = None;
        for block in self.doc.blocks() {
            match block {
                Block::Paragraph(paragraph) if paragraph.table_depth == 0 => {
                    out.push_str(&self.paragraph(paragraph, true));
                }
                Block::Paragraph(_) => {}
                Block::Table(table) => {
                    // A nested table also appears in the block list, inside its parent's range.
                    let nested = table_range.clone().is_some_and(|range| {
                        table.range.start >= range.start && table.range.end <= range.end
                    });
                    if !nested {
                        out.push_str(&self.table(table));
                        table_range = Some(table.range.clone());
                    }
                }
            }
        }
        out
    }

    /// One paragraph. `standalone` is `false` inside a table cell, where the paragraph keeps
    /// its items but always maps to `p`, a heading being meaningless in a cell.
    fn paragraph(&mut self, paragraph: &Paragraph, standalone: bool) -> String {
        let level = standalone
            .then_some(paragraph.style.as_deref())
            .flatten()
            .and_then(heading_level);
        let tag = match level {
            Some(level) => {
                if self.inferred_title.is_none() {
                    self.inferred_title = Some(paragraph.text());
                }
                match level {
                    1 => "h1",
                    2 => "h2",
                    3 => "h3",
                    4 => "h4",
                    5 => "h5",
                    _ => "h6",
                }
            }
            None => "p",
        };
        let mut inner = String::new();
        for item in &paragraph.items {
            inner.push_str(&self.item(item));
        }
        if inner.trim().is_empty() {
            return String::new();
        }
        format!("<{tag}>{inner}</{tag}>\n")
    }

    /// One table, rebuilt from its rows and cells: each `w:tr` of this table a `<tr>`, each
    /// `w:tc` a `<td>` holding the blocks the index places inside it.
    fn table(&mut self, table: &Table) -> String {
        let xml = self.doc.document_xml().to_owned();
        let mut out = String::from("<table>\n");
        for row in rows_of(&xml, table.range.clone()) {
            out.push_str("<tr>");
            for cell in row {
                out.push_str("<td>");
                out.push_str(&self.cell_contents(cell));
                out.push_str("</td>");
            }
            out.push_str("</tr>\n");
        }
        out.push_str("</table>\n");
        out
    }

    /// The blocks inside one table cell: paragraphs, and nested tables held by the cell.
    fn cell_contents(&mut self, cell: Range<usize>) -> String {
        let blocks: Vec<Block> = self
            .doc
            .blocks()
            .iter()
            .filter(|block| {
                block_range(block)
                    .is_some_and(|range| range.start >= cell.start && range.end <= cell.end)
            })
            .cloned()
            .collect();
        // Nested tables swallow their own paragraphs, so those are not written twice.
        let nested: Vec<Range<usize>> = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Table(table) => Some(table.range.clone()),
                _ => None,
            })
            .collect();
        let mut out = String::new();
        for block in &blocks {
            match block {
                Block::Paragraph(paragraph) => {
                    let inside_nested = nested.iter().any(|range| {
                        paragraph.range.start >= range.start && paragraph.range.end <= range.end
                    });
                    if !inside_nested {
                        out.push_str(&self.paragraph(paragraph, false));
                    }
                }
                Block::Table(table) => out.push_str(&self.table(table)),
            }
        }
        // Paragraphs close with a newline for the page's own line breaks; inside a cell that
        // would push the closing `</td>` onto its own line, so the cell trims it back.
        out.trim_end().to_owned()
    }

    /// One paragraph item.
    fn item(&mut self, item: &Item) -> String {
        match item {
            Item::Text { text, style, .. } => styled_text(text, style),
            Item::OMath { range, display } => {
                let math = self.equation_math(item, Some(range));
                self.formula(math.as_ref(), *display)
            }
            Item::Object { .. } => {
                let math = self.equation_math(item, None);
                self.formula(math.as_ref(), false)
            }
            Item::Image { rel_id, name, .. } => self.picture(rel_id.as_deref(), name.as_deref()),
            Item::Other { range } => {
                let text = other_text(&self.doc.document_xml()[range.clone()]);
                if text.is_empty() {
                    String::new()
                } else {
                    escape(&text)
                }
            }
        }
    }

    /// The tree behind a formula item: the equation index first — it holds embedded objects
    /// too — and, for Office Math the index could not read, the markup itself.
    fn equation_math(&self, item: &Item, range: Option<&Range<usize>>) -> Option<Math> {
        if let Some(key) = self.equation_key(item) {
            if let Some(math) = self.equations.get(&key).cloned().flatten() {
                return Some(math);
            }
        }
        range.and_then(|range| mt_omml::parse(&self.doc.document_xml()[range.clone()]).ok())
    }

    /// The (block, item) coordinates of an item, by comparing edit ranges.
    fn equation_key(&self, target: &Item) -> Option<(usize, usize)> {
        let target_range = target.edit_range();
        self.doc.paragraphs().find_map(|(block, paragraph)| {
            paragraph
                .items
                .iter()
                .position(|item| item.edit_range() == target_range)
                .map(|item| (block, item))
        })
    }

    /// One formula, as MathML or as a picture.
    fn formula(&mut self, math: Option<&Math>, display: bool) -> String {
        let Some(math) = math else {
            return String::new();
        };
        match self.options.math {
            MathMode::MathMl => inline_mathml(math, display),
            MathMode::Image => {
                let alt = mt_tex::to_latex(math);
                let src = self.equation_picture(math);
                if src.is_empty() {
                    return String::new();
                }
                let mut out = format!("<img class=\"formula\" src=\"{src}\" alt=\"");
                out.push_str(&escape(&alt));
                out.push_str("\">");
                if display {
                    out = format!("<div class=\"formula-display\">{out}</div>");
                }
                out
            }
        }
    }

    /// One picture item: the document part's bytes, as a file or a data URI.
    fn picture(&mut self, rel_id: Option<&str>, name: Option<&str>) -> String {
        let Some(rel_id) = rel_id else {
            return String::new();
        };
        let Some(part) = self.doc.relationship_part(rel_id) else {
            return String::new();
        };
        let Some(Ok(bytes)) = self.doc.part(&part) else {
            return String::new();
        };
        let extension = part
            .rsplit('.')
            .next()
            .unwrap_or("bin")
            .to_ascii_lowercase();
        if self.options.embed {
            return format!("data:{};base64,{}", mime_of(&extension), base64(&bytes));
        }
        let media_path = match self.images.get(&part) {
            Some(path) => path.clone(),
            None => {
                let stem = name
                    .map(|name| {
                        name.chars()
                            .filter(|c| c.is_alphanumeric() || *c == '-')
                            .collect::<String>()
                    })
                    .filter(|stem| !stem.is_empty())
                    .unwrap_or_else(|| {
                        part.rsplit('/')
                            .next()
                            .unwrap_or("image")
                            .trim_end_matches(&format!(".{extension}"))
                            .to_owned()
                    });
                let path = format!("media/{stem}.{extension}");
                self.media.push(MediaFile {
                    path: path.clone(),
                    bytes,
                });
                self.images.insert(part, path.clone());
                path
            }
        };
        let alt = escape(name.unwrap_or("picture"));
        format!("<img src=\"{media_path}\" alt=\"{alt}\">")
    }

    /// One equation picture, rendered from the tree.
    fn equation_picture(&mut self, math: &Math) -> String {
        self.equation_count += 1;
        let path = format!("media/equation{}.png", self.equation_count);
        let metrics = FaceMetrics::parse(MATH_FONT, 0).expect("the bundled math font parses");
        let style = LayoutStyle::display(18.0);
        let tree: LayoutBox = layout(math, &metrics, &style);
        let options = RenderOptions {
            scale: 3.0,
            ..Default::default()
        };
        let Ok(bytes) = render_png(&tree, &style, metrics.face(), &options) else {
            return String::new();
        };
        if self.options.embed {
            return format!("data:image/png;base64,{}", base64(&bytes));
        }
        self.media.push(MediaFile {
            path: path.clone(),
            bytes,
        });
        path
    }
}

/// The `h1`–`h6` level of a paragraph style name, `Heading1` for instance.
fn heading_level(style: &str) -> Option<u8> {
    let digits: String = style.chars().filter(|c| c.is_ascii_digit()).collect();
    match digits.parse::<u8>() {
        Ok(level) if (1..=6).contains(&level) => Some(level),
        _ => None,
    }
}

/// One text run wrapped in the elements its style asks for.
fn styled_text(text: &str, style: &RunStyle) -> String {
    let mut out = escape(text);
    match style.vert_align {
        Some(VertAlign::Superscript) => out = format!("<sup>{out}</sup>"),
        Some(VertAlign::Subscript) => out = format!("<sub>{out}</sub>"),
        None => {}
    }
    if style.italic == Some(true) {
        out = format!("<em>{out}</em>");
    }
    if style.bold == Some(true) {
        out = format!("<strong>{out}</strong>");
    }
    out
}

/// The MathML of a tree, with the `m:` prefix removed — HTML5 MathML is unprefixed, and a
/// prefixed `m:math` element does not render as math in a browser — and the display attribute
/// set to the form the item had in the document.
fn inline_mathml(math: &Math, display: bool) -> String {
    let written = write_mathml(math)
        .replace("<m:", "<")
        .replace("</m:", "</")
        .replace(" xmlns:m=\"http://www.w3.org/1998/Math/MathML\"", "");
    if display {
        written
    } else {
        written.replacen("display=\"block\"", "display=\"inline\"", 1)
    }
}

/// The visible text of an unrecognised item: the characters of the `w:t` elements in its bytes.
fn other_text(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<w:t") {
        rest = &rest[start..];
        let Some(after_open) = rest.find('>') else {
            break;
        };
        let self_closing = rest[..after_open].ends_with('/');
        rest = &rest[after_open + 1..];
        if self_closing {
            continue;
        }
        let Some(end) = rest.find("</w:t>") else {
            break;
        };
        out.push_str(&unescape(&rest[..end]));
        rest = &rest[end + "</w:t>".len()..];
    }
    out
}

/// The basic XML entities, written back to characters.
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// The characters a text node of HTML cannot carry raw.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// The script tags of a rendering choice, or an empty string.
fn script_tag(script: Script) -> &'static str {
    match script {
        Script::None => "",
        Script::MathJax => concat!(
            "<script defer src=\"https://cdn.jsdelivr.net/npm/mathjax@3/es5/mml-chtml.js\">",
            "</script>\n"
        ),
        Script::KaTeX => concat!(
            "<link rel=\"stylesheet\" href=\"https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.css\">\n",
            "<script defer src=\"https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.js\"></script>\n",
            "<script defer src=\"https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/contrib/auto-render.min.js\"",
            " onload=\"renderMathInElement(document.body)\"></script>\n"
        ),
    }
}

/// The style sheet every exported page carries: a readable body, ruled tables, centred display
/// formulas, and pictures that never overflow the column.
const STYLE: &str = concat!(
    "<style>\n",
    "body { font-family: sans-serif; max-width: 46rem; margin: 2em auto; padding: 0 1em; line-height: 1.6; }\n",
    "h1, h2, h3, h4, h5, h6 { line-height: 1.25; }\n",
    "table { border-collapse: collapse; margin: 1em 0; }\n",
    "td, th { border: 1px solid #888; padding: 0.3em 0.6em; }\n",
    "img { max-width: 100%; }\n",
    "math[display=\"block\"] { display: block; margin: 0.6em auto; }\n",
    ".formula-display { text-align: center; margin: 0.6em 0; }\n",
    "</style>\n"
);

/// The byte range of a block's markup.
fn block_range(block: &Block) -> Option<Range<usize>> {
    match block {
        Block::Paragraph(paragraph) => Some(paragraph.range.clone()),
        Block::Table(table) => Some(table.range.clone()),
    }
}

// ------------------------------------------------------------------ the table scan

/// The rows of one table, each a list of cell ranges, in order.
///
/// Nested tables are skipped — they are blocks of their own, and the cell walk renders them
/// separately — so only the rows and cells of this table are reported.
fn rows_of(xml: &str, table: Range<usize>) -> Vec<Vec<Range<usize>>> {
    let mut rows: Vec<Vec<Range<usize>>> = Vec::new();
    let mut table_depth = 0usize;
    let mut cells: Vec<Range<usize>> = Vec::new();
    let mut open_cells: Vec<usize> = Vec::new();
    let mut offset = table.start;
    while let Some(tag) = next_tag(xml, offset, table.end) {
        offset = tag.range.end;
        match (tag.name(xml), tag.closes) {
            ("tbl", false) => table_depth += 1,
            ("tbl", true) => table_depth = table_depth.saturating_sub(1),
            ("tr", false) if table_depth == 1 => cells = Vec::new(),
            ("tr", true) if table_depth == 1 => {
                rows.push(std::mem::take(&mut cells));
                open_cells.clear();
            }
            ("tc", false) if table_depth == 1 => open_cells.push(tag.range.start),
            ("tc", true) if table_depth == 1 => {
                if let Some(start) = open_cells.pop() {
                    cells.push(start..tag.range.end);
                }
            }
            _ => {}
        }
    }
    rows
}

/// One opening or closing tag found in `xml[start..limit]`.
struct Tag {
    range: Range<usize>,
    closes: bool,
}

impl Tag {
    /// The tag's element name, without its namespace prefix.
    fn name<'x>(&self, xml: &'x str) -> &'x str {
        let name_start = self.range.start + if self.closes { 2 } else { 1 };
        let inner = &xml[name_start..self.range.end];
        let name = inner
            .split(|ch: char| ch.is_whitespace() || ch == '/' || ch == '>')
            .next()
            .unwrap_or(inner);
        name.rsplit(':').next().unwrap_or(name)
    }
}

/// The next `<...>` stretch after `start`, at or before `limit`.
fn next_tag(xml: &str, start: usize, limit: usize) -> Option<Tag> {
    let open = xml[start..limit].find('<')? + start;
    let close = xml[open..limit].find('>')? + open;
    let closes = xml[open..].starts_with("</");
    Some(Tag {
        range: open..close + 1,
        closes,
    })
}

// ------------------------------------------------------------------ small encoders

/// The MIME type of a picture extension.
fn mime_of(extension: &str) -> &'static str {
    match extension {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

/// Standard base64, written here rather than pulled in as a dependency for two media shapes.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let byte = |index: usize| chunk.get(index).copied().unwrap_or(0) as u32;
        let group = (byte(0) << 16) | (byte(1) << 8) | byte(2);
        for index in 0..4 {
            if index * 6 < chunk.len() * 8 {
                out.push(ALPHABET[(group >> (18 - index * 6) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn headings_map_to_levels() {
        assert_eq!(heading_level("Heading1"), Some(1));
        assert_eq!(heading_level("Heading6"), Some(6));
        assert_eq!(heading_level("Heading7"), None);
        assert_eq!(heading_level("Normal"), None);
    }

    #[test]
    fn text_is_escaped_and_wrapped_innermost_out() {
        let style = RunStyle {
            bold: Some(true),
            italic: Some(true),
            vert_align: Some(VertAlign::Superscript),
            size_half_points: None,
        };
        assert_eq!(
            styled_text("a<b & c", &style),
            "<strong><em><sup>a&lt;b &amp; c</sup></em></strong>"
        );
    }

    #[test]
    fn other_items_keep_their_text() {
        assert_eq!(
            other_text("<w:fldSimple><w:r><w:t>a &amp; b</w:t></w:r></w:fldSimple>"),
            "a & b"
        );
    }

    #[test]
    fn mathml_loses_its_prefix_and_honours_the_form() {
        let math = mt_tex::parse("x^2").unwrap();
        let inline = inline_mathml(&math, false);
        assert!(inline.starts_with("<math"), "{inline}");
        assert!(inline.contains("display=\"inline\""), "{inline}");
        assert!(!inline.contains("m:"), "{inline}");
        let display = inline_mathml(&math, true);
        assert!(display.contains("display=\"block\""), "{display}");
    }

    #[test]
    fn tables_are_scanned_row_by_row() {
        let xml = "<w:tbl><w:tr><w:tc><w:p>a</w:p></w:tc><w:tc>b</w:tc></w:tr>\
                   <w:tr><w:tc><w:tbl><w:tr><w:tc>inner</w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl>";
        let rows = rows_of(xml, 0..xml.len());
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0].len(), 2);
        assert_eq!(
            rows[1].len(),
            1,
            "the nested table's cell is not this table's cell"
        );
    }
}
