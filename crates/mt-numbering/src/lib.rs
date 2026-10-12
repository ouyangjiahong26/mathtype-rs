//! Equation numbering and cross-references for Word documents, built from Word fields (#11).
//!
//! Numbering is not computed here — it is *asked of Word*. Every number is a SEQ field, every
//! chapter/section label reads counters that chapter/section break fields reset, and every
//! cross-reference is a REF field pointing at the bookmark wrapped around a number. Word (or
//! any host with a field engine, such as `wordcraft-cli`) recomputes the text when fields are
//! updated, so numbers stay correct after the document is closed, reopened, and edited.
//!
//! The field spellings, in OOXML terms:
//!
//! * **Numbering** — the composite of the issue, as a complex field: a `MACROBUTTON MTPlaceRef`
//!   wrapper whose display is `{SEQ MTEqn \h}` (a hidden per-equation index the macro can read)
//!   followed by the visible label. The label comes in four shapes, one per [`NumberStyle`]:
//!   plain `(n)`, `chapter` `(c.n)`, `section` `(c.s.n)` — the full triple of the issue — and
//!   `paragraph` `(n)`, where `\s 1` makes Word restart the counter at every Heading 1.
//!   Chapter and section counters are read with `\c` (repeat the most recent number without
//!   incrementing): they are advanced only by the break fields, so the label cannot drift.
//! * **Breaks** — a hidden paragraph of SEQ fields with `\r`: `SEQ MTChap \r k` sets the chapter
//!   counter, `SEQ MTSec \r k` the section counter, and `SEQ MTEqn \r 0` restarts the equation
//!   counter, so the first equation after a break is number 1 of the new chapter/section.
//! * **Cross-reference** — `REF <bookmark> \h` on the `_RefMTEqN` bookmark wrapped around each
//!   number, `\h` making it a live hyperlink; updating fields refreshes the text.
//!
//! The style and side choices persist as a hidden run inside the `MTNumberSettings` bookmark at
//! the head of the document — plain Word content, so it survives any host's resave (a custom
//! part or a foreign element in `settings.xml` would not reliably). Reading finds no marker
//! back, the defaults are plain numbering on the right.
//!
//! Enumeration ([`enumerate`]) scans `word/document.xml` for the numbering fields and reports
//! each with its document-order number and bookmark, which is how a caller picks a target for a
//! cross-reference.

#![forbid(unsafe_code)]

#[cfg(test)]
mod tests;

use mt_core::Math;
use mt_docx::{Document, DocxError, Position};

/// The SEQ counter that numbers equations.
const EQN_COUNTER: &str = "MTEqn";
/// The SEQ counter a chapter break advances.
const CHAPTER_COUNTER: &str = "MTChap";
/// The SEQ counter a section break advances.
const SECTION_COUNTER: &str = "MTSec";
/// The macrobutton wrapper that carries each number, as in the issue.
const PLACE_MACRO: &str = "MTPlaceRef";
/// Prefix of the bookmark wrapped around each number; cross-references point at it.
const BOOKMARK_PREFIX: &str = "_RefMTEq";
/// Name of the bookmark that carries the persisted settings.
const SETTINGS_BOOKMARK: &str = "MTNumberSettings";
/// Where the number's tab stop sits: the right margin of an A4 page with 3 cm side margins,
/// in twentieths of a point.
const MARGIN_TAB: &str = "8296";

/// How a number is composed. Each style is one spelling of the SEQ fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberStyle {
    /// `(n)` — a single counter over the whole document.
    Plain,
    /// `(c.n)` — chapter counter, a dot, equation counter.
    Chapter,
    /// `(c.s.n)` — the full composite of the issue: chapter, section, equation.
    Section,
    /// `(n)` restarted at every Heading 1, by SEQ's `\s 1`.
    Paragraph,
}

impl NumberStyle {
    /// The style a `plain|chapter|section|paragraph` word names.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "plain" => Some(Self::Plain),
            "chapter" => Some(Self::Chapter),
            "section" => Some(Self::Section),
            "paragraph" => Some(Self::Paragraph),
            _ => None,
        }
    }

    /// The word that names this style.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Chapter => "chapter",
            Self::Section => "section",
            Self::Paragraph => "paragraph",
        }
    }
}

/// Which side of the equation the number sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// The equation, a right tab, the number at the margin.
    Right,
    /// The number at the left margin, a tab, the equation.
    Left,
}

impl Side {
    /// The side a `right|left` word names.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "right" => Some(Self::Right),
            "left" => Some(Self::Left),
            _ => None,
        }
    }

    /// The word that names this side.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Left => "left",
        }
    }
}

/// A break that resets the counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakKind {
    /// Advance the chapter counter and restart the equation counter.
    Chapter,
    /// Advance the section counter and restart the equation counter.
    Section,
}

impl BreakKind {
    /// The SEQ counter this break advances.
    const fn counter(self) -> &'static str {
        match self {
            Self::Chapter => CHAPTER_COUNTER,
            Self::Section => SECTION_COUNTER,
        }
    }

    /// The word that names this break.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chapter => "chapter",
            Self::Section => "section",
        }
    }
}

/// One numbered equation found in a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumberedEquation {
    /// The number in document order, 1-based: the third numbering field found.
    pub number: usize,
    /// The block of the paragraph the field sits in.
    pub block: usize,
    /// The bookmark wrapped around the number, what a cross-reference points at.
    pub bookmark: String,
}

/// Which numbered equation a cross-reference points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceTarget {
    /// The most recently inserted numbering field.
    Latest,
    /// The field with this document-order number, 1-based.
    Number(usize),
}

// ------------------------------------------------------------------ field XML

/// Escapes the characters that cannot appear in XML character data.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A run of text, hidden when asked.
fn text_run(text: &str, hidden: bool) -> String {
    let properties = hidden
        .then(|| "<w:rPr><w:vanish/></w:rPr>".to_owned())
        .unwrap_or_default();
    format!(
        "<w:r>{properties}<w:t xml:space=\"preserve\">{}</w:t></w:r>",
        escaped(text)
    )
}

/// A run holding a tab character.
fn tab_run() -> String {
    "<w:r><w:tab/></w:r>".to_owned()
}

/// A run holding one piece of field instruction text.
fn instr_run(instr: &str) -> String {
    format!(
        "<w:r><w:instrText xml:space=\"preserve\">{}</w:instrText></w:r>",
        escaped(instr)
    )
}

/// A run holding one field character.
fn fldchar(kind: &str) -> String {
    format!("<w:r><w:fldChar w:fldCharType=\"{kind}\"/></w:r>")
}

/// A whole field as runs: instruction, separator, cached result, end.
fn field_runs(instr: &str, cached: &str, cached_hidden: bool) -> String {
    format!(
        "{}{}{}{}{}",
        fldchar("begin"),
        instr_run(instr),
        fldchar("separate"),
        text_run(cached, cached_hidden),
        fldchar("end")
    )
}

/// One SEQ sub-field of the visible label: ` SEQ <counter> <flags> `.
fn seq_field(counter: &str, flags: &str, cached: &str) -> String {
    field_runs(&format!(" SEQ {counter} {flags} "), cached, false)
}

/// The visible label of a numbering field, per style, without the wrapper.
fn label_runs(style: NumberStyle) -> String {
    match style {
        NumberStyle::Plain => format!(
            "{}{}{}",
            text_run("(", false),
            seq_field(EQN_COUNTER, "\\* ARABIC", "1"),
            text_run(")", false)
        ),
        NumberStyle::Chapter => format!(
            "{}{}{}{}{}",
            text_run("(", false),
            seq_field(CHAPTER_COUNTER, "\\c \\* ARABIC", "1"),
            text_run(".", false),
            seq_field(EQN_COUNTER, "\\* ARABIC", "1"),
            text_run(")", false)
        ),
        NumberStyle::Section => format!(
            "{}{}{}{}{}{}{}",
            text_run("(", false),
            seq_field(CHAPTER_COUNTER, "\\c \\* ARABIC", "1"),
            text_run(".", false),
            seq_field(SECTION_COUNTER, "\\c \\* ARABIC", "1"),
            text_run(".", false),
            seq_field(EQN_COUNTER, "\\* ARABIC", "1"),
            text_run(")", false)
        ),
        NumberStyle::Paragraph => format!(
            "{}{}{}",
            text_run("(", false),
            seq_field(EQN_COUNTER, "\\s 1 \\* ARABIC", "1"),
            text_run(")", false)
        ),
    }
}

/// The numbering field of the issue, as runs: a `MACROBUTTON MTPlaceRef` wrapper whose display
/// is the hidden equation index `{SEQ MTEqn \h}` followed by the style's label.
pub fn numbering_field_runs(style: NumberStyle) -> String {
    format!(
        "{}{}{}{}{}",
        fldchar("begin"),
        instr_run(&format!(" MACROBUTTON {PLACE_MACRO} ")),
        field_runs(&format!(" SEQ {EQN_COUNTER} \\h "), "1", true),
        label_runs(style),
        fldchar("end")
    )
}

/// The numbering field as a `w:fldSimple`, the spelling a single SEQ fits.
///
/// Only the plain and paragraph styles are one field; the chapter and section labels nest SEQ
/// inside SEQ, which `fldSimple` cannot express, so they ask for the complex form.
pub fn numbering_field_simple(style: NumberStyle) -> Option<String> {
    let instr = match style {
        NumberStyle::Plain => format!(" SEQ {EQN_COUNTER} \\* ARABIC "),
        NumberStyle::Paragraph => format!(" SEQ {EQN_COUNTER} \\s 1 \\* ARABIC "),
        _ => return None,
    };
    Some(format!(
        "<w:fldSimple w:instr=\"{}\"><w:r><w:t xml:space=\"preserve\">(1)</w:t></w:r></w:fldSimple>",
        escaped(&instr)
    ))
}

/// A bookmark wrapping `inner`.
pub fn bookmark_runs(name: &str, id: i32, inner: &str) -> String {
    format!(
        "<w:bookmarkStart w:id=\"{id}\" w:name=\"{}\"/>{}<w:bookmarkEnd w:id=\"{id}\"/>",
        escaped(name),
        inner
    )
}

/// The paragraph properties of a numbered paragraph: the tab stop that positions the number.
fn tabs_properties(side: Side) -> String {
    let tab = match side {
        Side::Right => format!("<w:tab w:val=\"right\" w:pos=\"{MARGIN_TAB}\"/>"),
        Side::Left => format!("<w:tab w:val=\"left\" w:pos=\"0\"/>"),
    };
    format!("<w:tabs>{tab}</w:tabs>")
}

/// The tab-stop properties alone, for splicing into an existing `w:pPr`.
pub fn tabs_runs(side: Side) -> String {
    tabs_properties(side)
}

/// A whole `w:pPr` with the tab stop, for a paragraph that has no properties.
pub fn tabs_paragraph_properties(side: Side) -> String {
    format!("<w:pPr>{}</w:pPr>", tabs_properties(side))
}

/// Equation, tab, and number as run-level markup, the number wrapped in its bookmark.
///
/// `math_xml` is a whole `m:oMath` element, as `mt_omml::write` produces.
pub fn numbered_inline(
    math_xml: &str,
    style: NumberStyle,
    side: Side,
    bookmark: &str,
    id: i32,
) -> String {
    let number = bookmark_runs(bookmark, id, &numbering_field_runs(style));
    match side {
        Side::Right => format!("{math_xml}{}{number}", tab_run()),
        Side::Left => format!("{number}{}{math_xml}", tab_run()),
    }
}

/// The paragraph of a numbered equation, tab stop included, ready to insert at
/// [`Position::BodyEnd`].
pub fn numbered_paragraph(
    math_xml: &str,
    style: NumberStyle,
    side: Side,
    bookmark: &str,
    id: i32,
) -> String {
    format!(
        "<w:p>{}{}</w:p>",
        tabs_paragraph_properties(side),
        numbered_inline(math_xml, style, side, bookmark, id)
    )
}

/// The same paragraph with the tab stop written into its properties — the shape a host that
/// inserts raw XML (`Range.InsertXML` in Word) needs, because there is no paragraph to borrow
/// properties from.
pub fn numbered_paragraph_standalone(
    math_xml: &str,
    style: NumberStyle,
    side: Side,
    bookmark: &str,
    id: i32,
) -> String {
    format!(
        "<w:p xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">{}{}</w:p>",
        tabs_paragraph_properties(side),
        numbered_inline(math_xml, style, side, bookmark, id)
    )
}

/// The hidden paragraph of reset fields a chapter or section break inserts.
///
/// `index` is the break's 1-based number among breaks of its kind, so the first chapter break
/// sets the chapter counter to 1, the second to 2, and so on; the equation counter is reset to
/// 0 so the next equation is number 1 of the new chapter or section.
pub fn break_paragraph(kind: BreakKind, index: usize) -> String {
    let counter = field_runs(
        &format!(" SEQ {} \\r {index} ", kind.counter()),
        &index.to_string(),
        true,
    );
    let equation = field_runs(&format!(" SEQ {EQN_COUNTER} \\r 0 "), "0", true);
    format!("<w:p>{counter}{equation}</w:p>")
}

/// The same break paragraph with namespaces declared, for raw-XML insertion into a host.
pub fn break_paragraph_standalone(kind: BreakKind, index: usize) -> String {
    format!(
        "<w:p xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">{}</w:p>",
        break_paragraph(kind, index)
            .trim_start_matches("<w:p>")
            .trim_end_matches("</w:p>")
    )
}

/// A cross-reference field as runs: ` REF <bookmark> \h ` with the target's number cached.
pub fn reference_runs(bookmark: &str, number: usize) -> String {
    field_runs(
        &format!(" REF {bookmark} \\h "),
        &format!("({number})"),
        false,
    )
}

// ------------------------------------------------------------------ scanning

/// A numbering field found while scanning, with where it sits.
struct Found {
    /// Byte offset of the field's instruction text.
    offset: usize,
    /// The bookmark open around it, when there is one.
    bookmark: Option<String>,
}

/// Whether an instruction text is a numbering field: the composite wrapper, or a SEQ MTEqn
/// that is not the hidden index inside the wrapper (`\h`) and not a reset (`\r`).
fn is_numbering_instr(instr: &str) -> bool {
    if instr.contains(PLACE_MACRO) {
        return true;
    }
    instr.contains("SEQ")
        && instr.contains(EQN_COUNTER)
        && !instr.contains("\\h")
        && !instr.contains("\\r")
}

/// Scans document XML for the numbering fields, in document order.
fn scan_numbered(xml: &str) -> Vec<Found> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().check_end_names = false;
    let mut found = Vec::new();
    // Bookmarks seen but not yet closed, most recent last: a number belongs to the bookmark
    // that wraps it.
    let mut open: Vec<(String, String)> = Vec::new();
    // The character data of the `w:instrText` currently being read, if one is.
    let mut instr: Option<String> = None;
    // One entry per field currently open, saying whether it is the MACROBUTTON wrapper: a SEQ
    // nested inside the wrapper is the label of an already-counted number, not a number.
    let mut fields: Vec<bool> = Vec::new();
    loop {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
        let offset = reader.buffer_position() as usize;
        match event {
            Event::Start(element) | Event::Empty(element) => match element.local_name().as_ref() {
                "bookmarkStart" => {
                    let mut name = None;
                    let mut id = None;
                    for attribute in element.attributes().flatten() {
                        let value = attribute.value.clone().into_owned();
                        match attribute.key.local_name().as_ref() {
                            "name" => name = Some(value),
                            "id" => id = Some(value),
                            _ => {}
                        }
                    }
                    if let (Some(name), Some(id)) = (name, id) {
                        open.push((name, id));
                    }
                }
                "bookmarkEnd" => {
                    let id = element
                        .attributes()
                        .flatten()
                        .find(|attribute| attribute.key.local_name().as_ref() == "id")
                        .map(|attribute| attribute.value.clone().into_owned());
                    if let Some(id) = id {
                        open.retain(|(_, open_id)| *open_id != id);
                    }
                }
                "instrText" => instr = Some(String::new()),
                "fldChar" => {
                    let kind = element
                        .attributes()
                        .flatten()
                        .find(|attribute| attribute.key.local_name().as_ref() == "fldCharType")
                        .map(|attribute| attribute.value.clone().into_owned());
                    match kind.as_deref() {
                        Some("begin") => fields.push(false),
                        Some("end") => {
                            fields.pop();
                        }
                        _ => {}
                    }
                }
                "fldSimple" => {
                    let instr = element
                        .attributes()
                        .flatten()
                        .find(|attribute| attribute.key.local_name().as_ref() == "instr")
                        .map(|attribute| attribute.value.clone().into_owned())
                        .unwrap_or_default();
                    if is_numbering_instr(&instr) {
                        found.push(Found {
                            offset,
                            bookmark: open.last().map(|(name, _)| name.clone()),
                        });
                    }
                }
                _ => {}
            },
            Event::Text(text) => {
                if let Some(instr) = instr.as_mut() {
                    instr.push_str(&text.xml10_content());
                }
            }
            Event::End(end) => {
                if end.local_name().as_ref() == "instrText" {
                    if let Some(text) = instr.take() {
                        if text.contains(PLACE_MACRO) {
                            if let Some(current) = fields.last_mut() {
                                *current = true;
                            }
                            found.push(Found {
                                offset,
                                bookmark: open.last().map(|(name, _)| name.clone()),
                            });
                        } else if is_numbering_instr(&text) && !fields.iter().any(|place| *place) {
                            found.push(Found {
                                offset,
                                bookmark: open.last().map(|(name, _)| name.clone()),
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    found
}

/// Every numbered equation in the document, in document order, with its block and bookmark.
pub fn enumerate(document: &Document) -> Vec<NumberedEquation> {
    let xml = document.document_xml();
    scan_numbered(xml)
        .into_iter()
        .enumerate()
        .map(|(index, found)| {
            let block = document
                .blocks()
                .iter()
                .enumerate()
                .filter(|(_, block)| {
                    let range = match block {
                        mt_docx::Block::Paragraph(paragraph) => Some(paragraph.range.clone()),
                        // A table's own range is not a place a field sits; its cell paragraphs
                        // are blocks of their own.
                        mt_docx::Block::Table(_) => None,
                    };
                    range.is_some_and(|range| range.contains(&found.offset))
                })
                .map(|(block, _)| block)
                .next()
                .unwrap_or(0);
            NumberedEquation {
                number: index + 1,
                block,
                bookmark: found.bookmark.unwrap_or_default(),
            }
        })
        .collect()
}

/// A bookmark name and id no existing bookmark uses: the next free `_RefMTEqN`.
pub fn unique_bookmark(document: &Document) -> (String, i32) {
    unique_bookmark_in_xml(document.document_xml())
}

/// The same against raw document XML, for hosts that hand the markup over as a string.
pub fn unique_bookmark_in_xml(xml: &str) -> (String, i32) {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().check_end_names = false;
    let mut max_id = 0;
    let mut max_number = 0;
    loop {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
        if let Event::Start(element) | Event::Empty(element) = event {
            if element.local_name().as_ref() == "bookmarkStart" {
                for attribute in element.attributes().flatten() {
                    let value = attribute.value.clone().into_owned();
                    match attribute.key.local_name().as_ref() {
                        "id" => {
                            if let Ok(id) = value.parse::<i32>() {
                                max_id = max_id.max(id);
                            }
                        }
                        "name" => {
                            if let Some(rest) = value.strip_prefix(BOOKMARK_PREFIX) {
                                if let Ok(number) = rest.parse::<u32>() {
                                    max_number = max_number.max(number);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    (format!("{BOOKMARK_PREFIX}{}", max_number + 1), max_id + 1)
}

/// How many breaks of a kind the document already holds, by counting the reset fields.
fn break_count(xml: &str, kind: BreakKind) -> usize {
    break_count_in_xml(xml, kind)
}

/// The same against raw document XML, for hosts that hand the markup over as a string.
pub fn break_count_in_xml(xml: &str, kind: BreakKind) -> usize {
    xml.matches(&format!(" SEQ {} \\r ", kind.counter()))
        .count()
}

/// The numbered equations of raw document XML: each with its document-order number and the
/// bookmark wrapped around it. Hosts that hold the document open hand `WordOpenXML` over
/// instead of the container, and pick a cross-reference target from this list.
pub fn numbered_in_xml(xml: &str) -> Vec<(usize, String)> {
    scan_numbered(xml)
        .into_iter()
        .enumerate()
        .map(|(index, found)| (index + 1, found.bookmark.unwrap_or_default()))
        .collect()
}

/// Whether a paragraph already carries a `w:tabs` element, matched by local name.
fn has_tabs(paragraph: &str) -> bool {
    paragraph.contains("w:tabs") || paragraph.contains(":tabs")
}

// ------------------------------------------------------------------ settings

/// The settings marker's text: `MTNumberStyle=<style>;MTNumberSide=<side>`.
fn settings_text(style: NumberStyle, side: Side) -> String {
    format!(
        "MTNumberStyle={};MTNumberSide={}",
        style.as_str(),
        side.as_str()
    )
}

/// The settings marker as run-level markup, hidden inside its bookmark.
fn settings_runs(style: NumberStyle, side: Side, id: i32) -> String {
    bookmark_runs(
        SETTINGS_BOOKMARK,
        id,
        &text_run(&settings_text(style, side), true),
    )
}

/// The style and side a document's settings marker records; plain on the right when absent.
///
/// The marker is one hidden run, so one search finds it; a host that split the run would lose
/// the marker, and the defaults answer instead — the worst outcome is renumbering in the
/// default style, never a broken document.
pub fn read_settings(document: &Document) -> (NumberStyle, Side) {
    parse_settings(document.document_xml())
}

/// The same against raw document XML, for hosts that hand the markup over as a string.
pub fn parse_settings(xml: &str) -> (NumberStyle, Side) {
    let Some(start) = xml.find("MTNumberStyle=") else {
        return (NumberStyle::Plain, Side::Right);
    };
    let value = xml[start..].split(['<', ';']).next().unwrap_or_default();
    let style = value
        .strip_prefix("MTNumberStyle=")
        .and_then(NumberStyle::parse);
    let side = xml[start..]
        .find("MTNumberSide=")
        .and_then(|side_at| xml[start + side_at..].split(['<', ';']).next())
        .and_then(|text| text.strip_prefix("MTNumberSide="))
        .and_then(Side::parse);
    (
        style.unwrap_or(NumberStyle::Plain),
        side.unwrap_or(Side::Right),
    )
}

/// Writes the settings marker, replacing an existing one.
pub fn write_settings(
    document: &mut Document,
    style: NumberStyle,
    side: Side,
) -> Result<(), DocxError> {
    let marker = settings_runs(style, side, 0);
    let xml = document.document_xml().to_owned();
    // An existing marker spans its bookmarkStart to its bookmarkEnd; find both by name.
    if let Some(tag) = xml.find(&format!("w:name=\"{SETTINGS_BOOKMARK}\"")) {
        let start = xml[..tag].rfind("<w:bookmarkStart").unwrap_or(tag);
        let id = xml[start..tag]
            .split_once("w:id=\"")
            .map(|(_, rest)| rest.split('"').next().unwrap_or("0").to_owned())
            .unwrap_or_else(|| "0".to_owned());
        let end_marker = format!("w:bookmarkEnd w:id=\"{id}\"");
        if let Some(end_tag) = xml.find(&end_marker) {
            let end = xml[end_tag..]
                .find('/')
                .map(|slash| end_tag + slash + 2)
                .unwrap_or(xml.len());
            let replacement = settings_runs(style, side, id.parse().unwrap_or(0));
            return document.splice_range(start..end, &replacement);
        }
    }
    // No marker yet: before the first paragraph's first item, at its end when it has none, or
    // in a paragraph of its own when the body has none at all.
    let first = document.paragraphs().next().map(|(block, _)| block);
    if let Some(block) = first {
        let paragraph = match &document.blocks()[block] {
            mt_docx::Block::Paragraph(paragraph) => paragraph,
            mt_docx::Block::Table(_) => unreachable!("paragraphs() yields only paragraphs"),
        };
        return if paragraph.items.is_empty() {
            document.insert_fragment(Position::ParagraphEnd { block }, &marker)
        } else {
            document.insert_fragment(Position::BeforeItem { block, item: 0 }, &marker)
        };
    }
    document.insert_fragment(Position::BodyEnd, &format!("<w:p>{marker}</w:p>"))
}

// ------------------------------------------------------------------ high level

/// Inserts a numbered equation: tab stop, equation, tab, and the numbering field in its
/// bookmark, laid out per `side`.
///
/// At [`Position::BodyEnd`] a new paragraph is built with the tab stop in its properties;
/// elsewhere the markup joins the paragraph the position names, whose properties gain the tab
/// stop when they lack one.
pub fn insert_numbered(
    document: &mut Document,
    position: Position,
    math: &Math,
    style: NumberStyle,
    side: Side,
) -> Result<(), DocxError> {
    let (bookmark, id) = unique_bookmark(document);
    match position {
        Position::BodyEnd => {
            let math_xml = mt_omml::write(math);
            let fragment = numbered_paragraph(&math_xml, style, side, &bookmark, id);
            document.insert_fragment(position, &fragment)
        }
        Position::ParagraphEnd { block }
        | Position::BeforeItem { block, .. }
        | Position::AfterItem { block, .. } => {
            ensure_tab_stop(document, block, side)?;
            let math_xml = mt_omml::write(math);
            let fragment = numbered_inline(&math_xml, style, side, &bookmark, id);
            document.insert_fragment(position, &fragment)
        }
        Position::ParagraphPropertiesStart { .. } | Position::AfterParagraph { .. } => {
            Err(DocxError::Xml(
                "numbering is content inside a paragraph; name a content position".to_owned(),
            ))
        }
    }
}

/// Gives a paragraph the tab stop that positions a number, when it lacks one.
///
/// The stop joins the paragraph's properties when there are any, and opens a `w:pPr` when
/// there are none. `w:tabs` sits early in the schema's property order, so it is put first in
/// an existing `w:pPr`; hosts read documents whose properties are out of order all the same,
/// and the tests re-parse every fragment this writes.
pub fn ensure_tab_stop(document: &mut Document, block: usize, side: Side) -> Result<(), DocxError> {
    let paragraph = document.paragraphs().find(|(index, _)| *index == block);
    let Some((_, paragraph)) = paragraph else {
        return Err(DocxError::Xml(format!(
            "the document has no paragraph block {block}"
        )));
    };
    if has_tabs(&document.document_xml()[paragraph.range.clone()]) {
        return Ok(());
    }
    match paragraph.properties_open_end {
        Some(_) => document.insert_fragment(
            Position::ParagraphPropertiesStart { block },
            &tabs_runs(side),
        ),
        None => document.insert_fragment(
            Position::ParagraphPropertiesStart { block },
            &tabs_paragraph_properties(side),
        ),
    }
}

/// Appends a number to every equation the document already holds — or only the one `only`
/// names, 1-based among equations — in reverse document order, so earlier coordinates stay
/// valid while later ones shift. Returns how many equations were numbered.
pub fn number_existing(
    document: &mut Document,
    style: NumberStyle,
    side: Side,
    only: Option<usize>,
) -> Result<usize, DocxError> {
    let equations = document.equations();
    let targets: Vec<_> = equations
        .into_iter()
        .enumerate()
        .filter(|(index, _)| only.is_none_or(|only| *index + 1 == only))
        .map(|(_, equation)| (equation.block, equation.item))
        .collect();
    let count = targets.len();
    // Reverse: splicing into a paragraph adds an item to it, shifting later items of the same
    // paragraph; numbering from the end backwards leaves every remaining target untouched.
    for (block, item) in targets.into_iter().rev() {
        ensure_tab_stop(document, block, side)?;
        let (bookmark, id) = unique_bookmark(document);
        let number = bookmark_runs(&bookmark, id, &numbering_field_runs(style));
        let fragment = match side {
            Side::Right => format!("{}{number}", tab_run()),
            Side::Left => format!("{number}{}", tab_run()),
        };
        let position = match side {
            Side::Right => Position::AfterItem { block, item },
            Side::Left => Position::BeforeItem { block, item },
        };
        document.insert_fragment(position, &fragment)?;
    }
    Ok(count)
}

/// Inserts a chapter or section break: the hidden paragraph of reset fields, at the end of the
/// body. The break's index is one past the breaks of its kind the document already holds.
pub fn insert_break(document: &mut Document, kind: BreakKind) -> Result<usize, DocxError> {
    let index = break_count(document.document_xml(), kind) + 1;
    document.insert_fragment(Position::BodyEnd, &break_paragraph(kind, index))?;
    Ok(index)
}

/// Inserts a cross-reference to a numbered equation: the REF field on the target's bookmark,
/// at the position named. Returns the bookmark the reference points at.
pub fn insert_reference(
    document: &mut Document,
    position: Position,
    target: ReferenceTarget,
) -> Result<String, DocxError> {
    let numbered = enumerate(document);
    let target = match target {
        ReferenceTarget::Latest => numbered.last(),
        ReferenceTarget::Number(number) => numbered.iter().find(|found| found.number == number),
    };
    let Some(target) = target else {
        return Err(DocxError::Xml(
            "the document has no numbered equation to reference".to_owned(),
        ));
    };
    let fragment = reference_runs(&target.bookmark, target.number);
    let fragment = match position {
        Position::BodyEnd => format!("<w:p>{fragment}</w:p>"),
        _ => fragment,
    };
    document.insert_fragment(position, &fragment)?;
    Ok(target.bookmark.clone())
}
