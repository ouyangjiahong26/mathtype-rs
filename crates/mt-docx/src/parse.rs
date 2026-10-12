//! The scanner that walks `word/document.xml` once and builds the document index.
//!
//! It never materialises a tree. It streams events, keeps a stack only as deep as the element it
//! is currently inside, and records byte ranges: a `w:p` becomes a [`Paragraph`] whose items each
//! remember where their markup starts and ends. Namespaces are matched by local name, so a
//! document is accepted whatever prefixes it binds.

use std::ops::Range;

use quick_xml::Reader;
use quick_xml::escape::{resolve_predefined_entity, unescape};
use quick_xml::events::{BytesStart, BytesText, Event};

use crate::error::{DocxError, xml_error};
use crate::model::{Block, Item, Paragraph, RunStyle, Table, VertAlign};

/// What a scan produces: the block list and the offset an editor splices at.
pub(crate) struct Scanned {
    /// Body-level blocks in document order, paragraphs inside tables included.
    pub blocks: Vec<Block>,
    /// The offset `</w:body>` starts at, the insertion point for a new block at the end.
    pub body_close: usize,
}

/// Scans `word/document.xml`.
pub(crate) fn scan(xml: &str) -> Result<Scanned, DocxError> {
    Scanner::new(xml).run()
}

struct Scanner<'a> {
    reader: Reader<&'a [u8]>,
    length: usize,
}

/// The next event and the byte range it occupies in the source.
type Stepped<'a> = (Event<'a>, Range<usize>);

impl<'a> Scanner<'a> {
    fn new(xml: &'a str) -> Self {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().check_end_names = true;
        reader.config_mut().trim_text_start = false;
        reader.config_mut().trim_text_end = false;
        Self {
            reader,
            length: xml.len(),
        }
    }

    fn next(&mut self) -> Result<Option<Stepped<'a>>, DocxError> {
        let start = self.reader.buffer_position() as usize;
        match self.reader.read_event().map_err(xml_error)? {
            Event::Eof => Ok(None),
            event => {
                let end = self.reader.buffer_position() as usize;
                Ok(Some((event, start..end)))
            }
        }
    }

    /// Consumes the rest of the element whose start tag has already been read, returning the
    /// offset its end tag ends at.
    fn close(&mut self) -> Result<usize, DocxError> {
        let mut depth = 0usize;
        loop {
            let Some((event, range)) = self.next()? else {
                return Err(DocxError::Xml("an element is never closed".to_owned()));
            };
            match event {
                Event::Start(_) => depth += 1,
                Event::End(_) => {
                    if depth == 0 {
                        return Ok(range.end);
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
    }

    /// Runs the whole scan: find the body, index its children, note where the body ends.
    fn run(mut self) -> Result<Scanned, DocxError> {
        let mut blocks = Vec::new();
        let mut body_close = self.length;
        while let Some((event, _)) = self.next()? {
            let Event::Start(start) = event else { continue };
            match start.local_name().as_ref() {
                // The document root's own start tag: its children are what follows.
                "document" => {}
                "body" => {
                    body_close = self.scan_body(&mut blocks)?;
                    break;
                }
                _ => {
                    let _ = self.close()?;
                }
            }
        }
        Ok(Scanned { blocks, body_close })
    }

    /// Indexes the children of `w:body` until its end tag; returns the offset that tag starts at.
    fn scan_body(&mut self, blocks: &mut Vec<Block>) -> Result<usize, DocxError> {
        loop {
            let Some((event, range)) = self.next()? else {
                return Ok(self.length);
            };
            match event {
                Event::Start(start) => match start.local_name().as_ref() {
                    "p" => {
                        let paragraph = self.scan_paragraph(range.start, range.end, 0)?;
                        blocks.push(Block::Paragraph(paragraph));
                    }
                    "tbl" => {
                        // The placeholder keeps the table's block in document order while its
                        // cell paragraphs, pushed as they are scanned, come after it.
                        blocks.push(Block::Table(Table {
                            range: 0..0,
                            rows: 0,
                            columns: 0,
                        }));
                        let index = blocks.len() - 1;
                        let table = self.scan_table(range.start, 0, blocks)?;
                        blocks[index] = Block::Table(table);
                    }
                    _ => {
                        let _ = self.close()?;
                    }
                },
                Event::Empty(start) if start.local_name().as_ref() == "p" => {
                    blocks.push(Block::Paragraph(empty_paragraph(&range)));
                }
                Event::End(end) if end.local_name().as_ref() == "body" => return Ok(range.start),
                _ => {}
            }
        }
    }

    /// Indexes a `w:tbl`: counts rows and columns, and feeds the paragraphs of its cells into the
    /// block list so formulas inside tables are found.
    fn scan_table(
        &mut self,
        start: usize,
        depth: usize,
        blocks: &mut Vec<Block>,
    ) -> Result<Table, DocxError> {
        let mut rows = 0usize;
        let mut grid_columns = 0usize;
        let mut row_columns = 0usize;
        let end = loop {
            let Some((event, range)) = self.next()? else {
                break self.length;
            };
            match event {
                Event::Start(start) => match start.local_name().as_ref() {
                    "tr" => {
                        rows += 1;
                        let cells = self.scan_row(depth, blocks)?;
                        row_columns = row_columns.max(cells);
                    }
                    "gridCol" => {
                        grid_columns += 1;
                        let _ = self.close()?;
                    }
                    _ => {
                        let _ = self.close()?;
                    }
                },
                Event::Empty(start) if start.local_name().as_ref() == "gridCol" => {
                    grid_columns += 1
                }
                Event::End(end) if end.local_name().as_ref() == "tbl" => break range.end,
                _ => {}
            }
        };
        Ok(Table {
            range: start..end,
            rows,
            columns: grid_columns.max(row_columns),
        })
    }

    /// Indexes a `w:tr`, returning how many cells it holds.
    fn scan_row(&mut self, depth: usize, blocks: &mut Vec<Block>) -> Result<usize, DocxError> {
        let mut cells = 0usize;
        while let Some((event, range)) = self.next()? {
            match event {
                Event::Start(start) if start.local_name().as_ref() == "tc" => {
                    cells += 1;
                    let _ = range;
                    self.scan_cell(depth + 1, blocks)?;
                }
                Event::End(end) if end.local_name().as_ref() == "tr" => break,
                _ => {}
            }
        }
        Ok(cells)
    }

    /// Indexes the paragraphs and nested tables of a `w:tc`.
    fn scan_cell(&mut self, depth: usize, blocks: &mut Vec<Block>) -> Result<(), DocxError> {
        while let Some((event, range)) = self.next()? {
            match event {
                Event::Start(start) => match start.local_name().as_ref() {
                    "p" => {
                        let paragraph = self.scan_paragraph(range.start, range.end, depth)?;
                        blocks.push(Block::Paragraph(paragraph));
                    }
                    "tbl" => {
                        blocks.push(Block::Table(Table {
                            range: 0..0,
                            rows: 0,
                            columns: 0,
                        }));
                        let index = blocks.len() - 1;
                        let table = self.scan_table(range.start, depth, blocks)?;
                        blocks[index] = Block::Table(table);
                    }
                    _ => {
                        let _ = self.close()?;
                    }
                },
                Event::Empty(start) if start.local_name().as_ref() == "p" => {
                    blocks.push(Block::Paragraph(empty_paragraph(&range)));
                }
                Event::End(end) if end.local_name().as_ref() == "tc" => break,
                _ => {}
            }
        }
        Ok(())
    }

    /// Indexes one `w:p`: its style, whether it is part of a list, and its item sequence.
    fn scan_paragraph(
        &mut self,
        start: usize,
        open_tag_end: usize,
        table_depth: usize,
    ) -> Result<Paragraph, DocxError> {
        let mut style = None;
        let mut numbered = false;
        let mut properties_open_end = None;
        let mut items: Vec<Item> = Vec::new();
        // The start of a stretch of unrecognised content, closed when a recognised item begins.
        let mut pending: Option<usize> = None;

        loop {
            let Some((event, range)) = self.next()? else {
                return Ok(Paragraph {
                    range: start..self.length,
                    open_tag_end,
                    properties_open_end,
                    content_end: self.length,
                    style,
                    numbered,
                    table_depth,
                    items,
                });
            };
            match event {
                Event::Start(start) => match start.local_name().as_ref() {
                    "pPr" => {
                        properties_open_end = Some(range.end);
                        pending.get_or_insert(range.start);
                        self.read_paragraph_properties(&mut style, &mut numbered)?;
                    }
                    "r" => {
                        let item = self.scan_run(range.start)?;
                        let item_end = item.edit_range().end;
                        close_pending(&mut pending, &mut items, range.start);
                        items.push(item);
                        pending = Some(item_end);
                    }
                    "oMath" | "oMathPara" => {
                        let display = start.local_name().as_ref() == "oMathPara";
                        let end = self.close()?;
                        close_pending(&mut pending, &mut items, range.start);
                        items.push(Item::OMath {
                            range: range.start..end,
                            display,
                        });
                        pending = Some(end);
                    }
                    _ => {
                        pending.get_or_insert(range.start);
                        let _ = self.close()?;
                    }
                },
                Event::Empty(start) => match start.local_name().as_ref() {
                    "oMath" | "oMathPara" => {
                        let display = start.local_name().as_ref() == "oMathPara";
                        close_pending(&mut pending, &mut items, range.start);
                        items.push(Item::OMath {
                            range: range.clone(),
                            display,
                        });
                        pending = Some(range.end);
                    }
                    _ => {
                        pending.get_or_insert(range.start);
                    }
                },
                Event::End(end) if end.local_name().as_ref() == "p" => {
                    // Unrecognised content ends where the closing tag begins, so the tag itself
                    // stays outside the items — and that offset is where a paragraph insert goes.
                    close_pending(&mut pending, &mut items, range.start);
                    break Ok(Paragraph {
                        range: start..range.end,
                        open_tag_end,
                        properties_open_end,
                        content_end: range.start,
                        style,
                        numbered,
                        table_depth,
                        items,
                    });
                }
                _ => {
                    pending.get_or_insert(range.start);
                }
            }
        }
    }

    /// Reads `w:pStyle` and the presence of `w:numPr` from a `w:pPr` the scanner just entered.
    fn read_paragraph_properties(
        &mut self,
        style: &mut Option<String>,
        numbered: &mut bool,
    ) -> Result<(), DocxError> {
        while let Some((event, _)) = self.next()? {
            match event {
                Event::Start(element) | Event::Empty(element) => {
                    match element.local_name().as_ref() {
                        "pStyle" => *style = attribute(&element, "val"),
                        "numPr" => *numbered = true,
                        _ => {}
                    }
                }
                Event::End(end) if end.local_name().as_ref() == "pPr" => break,
                _ => {}
            }
        }
        Ok(())
    }

    /// Indexes one `w:r`, classifying it by what it carries.
    fn scan_run(&mut self, start: usize) -> Result<Item, DocxError> {
        let mut run_style = RunStyle::default();
        let mut text = String::new();
        let mut object = None;
        let mut picture = None;
        let end = loop {
            let Some((event, range)) = self.next()? else {
                break self.length;
            };
            match event {
                Event::Start(element) => match element.local_name().as_ref() {
                    "rPr" => self.read_run_properties(&mut run_style)?,
                    "object" => object = Some(self.scan_object(range.start, range.end)?),
                    "drawing" | "pict" => picture = Some(self.scan_picture(range.start)?),
                    "t" => {
                        let piece = self.read_text()?;
                        text.push_str(&piece);
                    }
                    _ => {
                        let _ = self.close()?;
                    }
                },
                Event::Empty(element) if element.local_name().as_ref() == "t" => {}
                Event::End(end) if end.local_name().as_ref() == "r" => break range.end,
                _ => {}
            }
        };
        Ok(classify_run(start..end, run_style, text, object, picture))
    }

    /// Reads `w:rPr` into a style summary.
    fn read_run_properties(&mut self, style: &mut RunStyle) -> Result<(), DocxError> {
        while let Some((event, _)) = self.next()? {
            match event {
                Event::Start(element) | Event::Empty(element) => {
                    match element.local_name().as_ref() {
                        "b" => style.bold = Some(switch(&element, true)),
                        "i" => style.italic = Some(switch(&element, true)),
                        "vertAlign" => {
                            style.vert_align = match attribute(&element, "val").as_deref() {
                                Some("superscript") => Some(VertAlign::Superscript),
                                Some("subscript") => Some(VertAlign::Subscript),
                                _ => None,
                            }
                        }
                        "sz" | "szCs" if style.size_half_points.is_none() => {
                            style.size_half_points =
                                attribute(&element, "val").and_then(|value| value.parse().ok());
                        }
                        _ => {}
                    }
                }
                Event::End(end) if end.local_name().as_ref() == "rPr" => break,
                _ => {}
            }
        }
        Ok(())
    }

    /// Reads the character data of a `w:t` the scanner just entered.
    fn read_text(&mut self) -> Result<String, DocxError> {
        let mut text = String::new();
        while let Some((event, _)) = self.next()? {
            match event {
                Event::Text(raw) => text.push_str(&decoded(&raw)?),
                Event::GeneralRef(raw) => text.push_str(&reference(&raw)?),
                Event::End(end) if end.local_name().as_ref() == "t" => break,
                _ => {}
            }
        }
        Ok(text)
    }

    /// Indexes a `w:object` whose start tag begins at `start`, returning the element range, the
    /// markup that shows its preview, and the references it carries.
    ///
    /// The references sit at any depth — `o:OLEObject` at the top, `v:imagedata` inside the
    /// preview shape — so nested elements are walked rather than skipped.
    fn scan_object(&mut self, start: usize, open_end: usize) -> Result<ObjectFindings, DocxError> {
        let mut findings = ObjectFindings {
            element_range: start..self.length,
            preview_range: None,
            prog_id: None,
            data_rel_id: None,
            preview_rel_id: None,
        };
        let mut depth = 0usize;
        let end = loop {
            let Some((event, range)) = self.next()? else {
                break self.length;
            };
            match event {
                Event::Start(element) => {
                    depth += 1;
                    self.note_object(&element, &range, open_end, &mut findings);
                }
                Event::Empty(element) => {
                    self.note_object(&element, &range, open_end, &mut findings)
                }
                Event::End(_) => {
                    if depth == 0 {
                        break range.end;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        };
        findings.element_range = start..end;
        Ok(findings)
    }

    /// Notes one element of a `w:object`.
    fn note_object(
        &self,
        element: &quick_xml::events::BytesStart<'_>,
        range: &Range<usize>,
        open_end: usize,
        findings: &mut ObjectFindings,
    ) {
        match element.local_name().as_ref() {
            "OLEObject" => {
                findings.prog_id = attribute(element, "ProgID");
                findings.data_rel_id = attribute(element, "id");
                findings.preview_range = Some(open_end..range.start);
            }
            "imagedata" => findings.preview_rel_id = attribute(element, "id"),
            _ => {}
        }
    }

    /// Indexes a `w:drawing` or `w:pict` whose start tag begins at `start`, returning the element
    /// range and the relationship id of the picture it shows.
    ///
    /// The picture's reference sits deep inside DrawingML wrappers, so nested elements are
    /// walked rather than skipped.
    fn scan_picture(&mut self, start: usize) -> Result<PictureFindings, DocxError> {
        let mut findings = PictureFindings {
            element_range: start..self.length,
            rel_id: None,
            name: None,
        };
        let mut depth = 0usize;
        let end = loop {
            let Some((event, range)) = self.next()? else {
                break self.length;
            };
            match event {
                Event::Start(element) => {
                    depth += 1;
                    note_picture(&element, &mut findings);
                }
                Event::Empty(element) => note_picture(&element, &mut findings),
                Event::End(_) => {
                    if depth == 0 {
                        break range.end;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        };
        findings.element_range = start..end;
        Ok(findings)
    }
}

/// What the scanner found inside a `w:object`.
struct ObjectFindings {
    element_range: Range<usize>,
    preview_range: Option<Range<usize>>,
    prog_id: Option<String>,
    data_rel_id: Option<String>,
    preview_rel_id: Option<String>,
}

/// What the scanner found inside a `w:drawing` or `w:pict`.
struct PictureFindings {
    element_range: Range<usize>,
    rel_id: Option<String>,
    name: Option<String>,
}

/// An empty paragraph, from a self-closed `<w:p/>`.
fn empty_paragraph(range: &Range<usize>) -> Paragraph {
    Paragraph {
        range: range.clone(),
        open_tag_end: range.end,
        properties_open_end: None,
        content_end: range.end,
        style: None,
        numbered: false,
        table_depth: 0,
        items: Vec::new(),
    }
}

/// Closes the pending stretch of unrecognised content, if one is open.
fn close_pending(pending: &mut Option<usize>, items: &mut Vec<Item>, end: usize) {
    if let Some(start) = pending.take() {
        if start < end {
            items.push(Item::Other { range: start..end });
        }
    }
}

/// Notes one element of a `w:drawing` or `w:pict`.
fn note_picture(element: &quick_xml::events::BytesStart<'_>, findings: &mut PictureFindings) {
    match element.local_name().as_ref() {
        "blip" => findings.rel_id = attribute(element, "embed"),
        "imagedata" => findings.rel_id = attribute(element, "id"),
        "docPr" => findings.name = attribute(element, "name"),
        _ => {}
    }
}

/// Decides what a run is by what the scanner found inside it.
fn classify_run(
    range: Range<usize>,
    style: RunStyle,
    text: String,
    object: Option<ObjectFindings>,
    picture: Option<PictureFindings>,
) -> Item {
    if let Some(findings) = object {
        return Item::Object {
            run_range: range,
            range: findings.element_range,
            preview_range: findings.preview_range,
            prog_id: findings.prog_id,
            data_rel_id: findings.data_rel_id,
            preview_rel_id: findings.preview_rel_id,
        };
    }
    if let Some(findings) = picture {
        return Item::Image {
            run_range: range,
            range: findings.element_range,
            rel_id: findings.rel_id,
            name: findings.name,
        };
    }
    if text.is_empty() {
        return Item::Other { range };
    }
    Item::Text { range, text, style }
}

/// An attribute value, unescaped, matched by local name.
fn attribute(element: &BytesStart<'_>, name: &str) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.local_name().as_ref() == name)
        .and_then(|attribute| {
            unescape(&attribute.value)
                .ok()
                .map(|value| value.into_owned())
        })
}

/// An on/off property: on unless the value spells off.
fn switch(element: &BytesStart<'_>, on: bool) -> bool {
    match attribute(element, "val").as_deref() {
        Some("0") | Some("false") | Some("off") | Some("none") => !on,
        _ => on,
    }
}

/// Character data of a text event, entities resolved.
fn decoded(raw: &BytesText<'_>) -> Result<String, DocxError> {
    let content = raw.xml10_content();
    Ok(unescape(&content).map_err(xml_error)?.into_owned())
}

/// A general entity reference (`&amp;`, `&#x1D400;`), given without its `&` and `;`, as characters.
fn reference(raw: &quick_xml::events::BytesRef<'_>) -> Result<String, DocxError> {
    let name = raw.xml10_content();
    let resolved = if let Some(digits) = name.strip_prefix('#') {
        match digits.strip_prefix(['x', 'X']) {
            Some(hexadecimal) => u32::from_str_radix(hexadecimal, 16).ok(),
            None => digits.parse::<u32>().ok(),
        }
        .and_then(char::from_u32)
        .map(String::from)
    } else {
        resolve_predefined_entity(&name).map(str::to_owned)
    };
    resolved.ok_or_else(|| DocxError::Xml(format!("&{name}; is not a known entity")))
}
