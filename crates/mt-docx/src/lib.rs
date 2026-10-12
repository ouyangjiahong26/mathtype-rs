//! Reader, light index, and editor for the parts of a Word document the equation tools touch.
//!
//! # 设计取向：轻索引 + 原始片段
//!
//! This crate does **not** build a document model. It scans `word/document.xml` once and records
//! a light index — the block sequence (paragraphs and tables) and, inside each paragraph, the
//! item sequence: text runs, Office Math formulas, embedded objects, pictures, and everything
//! else kept as its original byte range. To edit, it splices generated or caller-given XML in at
//! those ranges and rescans; to export, a consumer reads the index and slices out the raw
//! fragments it wants to map. Unrecognised structure — headings, lists, tables, fields — is
//! never rebuilt, so it survives an edit byte-for-byte, and parts of the container this crate was
//! not asked to touch are copied verbatim on write.
//!
//! That buys the three consumers this crate serves:
//!
//! * **网页导出 (#18)** — walk [`Document::blocks`], map text and formula items, slice out the
//!   raw fragments of tables and anything unusual.
//! * **文档级批量转换 (#13)** — [`Document::equations`] locates every formula with its tree,
//!   [`Document::replace`] rewrites one item at a time, [`Document::into_bytes`] writes back.
//! * **公式编号 (#11)** — [`Document::insert_field`] splices field markup (`fldSimple` or an
//!   `instrText` run, given as raw XML) in next to a formula; [`Document::insert_omath`] adds a
//!   new formula.
//!
//! # Formulas
//!
//! Office Math items carry their markup in `word/document.xml` and are converted with `mt-omml`.
//! Embedded objects are OLE compound files in `word/embeddings/`; their `Equation Native` stream
//! is converted with `mt-mtef`. Replacing an object's equation rewrites the stream inside the
//! existing compound file, so its other streams survive; replacing a formula with a new object
//! builds a minimal compound file around a fresh stream (see [`Replacement::Ole`]).
//!
//! # 边界：不做与忽略
//!
//! Not done, by design — such markup is carried through unchanged when it is not the target of
//! an edit:
//!
//! * 域引擎：fields are never interpreted. `fldSimple` and `instrText` are inserted from raw XML
//!   given by the caller and read back only as opaque content.
//! * 样式解析重建：`styles.xml` and the rest of the container are copied verbatim; the index
//!   keeps a paragraph's style *name* and a run's style *summary*, nothing more.
//! * 跟踪修订 (tracked changes)、`doc` 旧格式：ignored.
//!
//! Folded into the opaque `Other` items of the index, so their formulas are not enumerated:
//! hyperlink bodies, `mc:AlternateContent` fallbacks, and paragraphs inside text boxes. Body and
//! table-cell paragraphs — where formulas live in practice — are all indexed.

#![forbid(unsafe_code)]

mod container;
mod content_types;
mod error;
mod fragment;
mod model;
mod parse;
mod rels;

pub use container::Package;
pub use error::DocxError;
pub use model::{Block, Equation, EquationKind, Item, Paragraph, RunStyle, Table, VertAlign};
pub use rels::{Relationship, Relationships};

use std::ops::Range;

use mt_core::Math;

use crate::content_types::ContentTypes;
use crate::rels::kinds;

/// The main document part, the one this crate indexes and edits.
const DOCUMENT_PART: &str = "word/document.xml";
/// The relationships of the main document part, the ones its `r:id` references hang from.
const DOCUMENT_RELS_PART: &str = "word/_rels/document.xml.rels";
/// The container's content type table.
const CONTENT_TYPES_PART: &str = "[Content_Types].xml";
/// The ProgID written for a freshly embedded equation object.
const DEFAULT_PROG_ID: &str = "Equation.3";
/// The content type of an OLE object part.
const OLE_CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.oleObject";

/// A picture to place in the document.
pub struct ImageData {
    /// The picture bytes.
    pub bytes: Vec<u8>,
    /// The file extension, `png` for instance.
    pub extension: String,
    /// The content type, `image/png` for instance.
    pub content_type: String,
    /// The name shown for the drawing.
    pub name: String,
    /// The size, in English Metric Units: 914400 per inch.
    pub extent_emu: (u32, u32),
}

/// What an item is replaced with, in [`Document::replace`].
pub enum Replacement {
    /// An Office Math formula generated from a tree.
    OMath {
        /// The equation.
        math: Math,
        /// The display form (`m:oMathPara`) when true, inline (`m:oMath`) when false.
        display: bool,
    },
    /// A run of text, optionally styled.
    Text {
        /// The characters.
        text: String,
        /// The style summary to write; only what is set is written.
        style: RunStyle,
    },
    /// A picture: a new media part, a relationship, and an inline drawing.
    Image(ImageData),
    /// An embedded equation object: an OLE compound file holding the equation as its
    /// `Equation Native` stream.
    ///
    /// When the item being replaced is already an object, the stream is rewritten inside the
    /// existing compound file — its other streams, and the markup that shows its preview, are
    /// kept. Otherwise a minimal compound file is added as a new part, with no preview picture:
    /// hosts open such an object and show a placeholder box until the object is edited once.
    Ole {
        /// The equation.
        math: Math,
        /// The ProgID of a newly created object; the object's own when one already exists.
        /// `Equation.DSMT4` makes the object MathType's own, so MathType users can edit it.
        prog_id: Option<String>,
        /// A preview picture for a newly created object, shown by hosts that cannot run the
        /// equation server; an existing object keeps the preview it already has.
        preview: Option<ImageData>,
    },
    /// Raw XML spliced in verbatim. The caller owns well-formedness and namespaces.
    Raw(String),
}

/// Where an insertion goes, in [`Document::insert_omath`] and friends.
#[derive(Clone, Copy, Debug)]
pub enum Position {
    /// At the end of the body, as a new paragraph.
    BodyEnd,
    /// At the end of a paragraph, before its closing tag.
    ParagraphEnd { block: usize },
    /// Immediately before an item.
    BeforeItem { block: usize, item: usize },
    /// Immediately after an item.
    AfterItem { block: usize, item: usize },
    /// Where a paragraph's properties go: right after the `w:pPr` opening tag when the paragraph
    /// has properties, otherwise right after the paragraph's opening tag, where a new `w:pPr`
    /// begins. The XML must therefore be property elements (`<w:tabs/>`, say) in the first case
    /// and a whole `w:pPr` in the second.
    ParagraphPropertiesStart { block: usize },
    /// After the paragraph's closing tag: whole paragraphs (`<w:p>…</w:p>`) go here.
    AfterParagraph { block: usize },
}

/// A Word document opened for reading and editing.
///
/// One [`Document`] owns the whole container, the scanned index of the main part, and the
/// relationship and content-type tables. Edits rescan the index, so block and item numbers —
/// the coordinates everything is addressed by — stay valid from one edit to the next and are
/// invalidated by it: enumerate, edit, re-enumerate.
pub struct Document {
    package: Package,
    xml: String,
    blocks: Vec<Block>,
    body_close: usize,
    rels: Relationships,
    content_types: ContentTypes,
    dirty_xml: bool,
    dirty_rels: bool,
    dirty_content_types: bool,
    /// Ids for generated `wp:docPr` and `o:OLEObject` markup, seeded past what documents write.
    next_generated_id: u32,
}

impl Document {
    /// Opens a container and scans its main part.
    pub fn open(bytes: &[u8]) -> Result<Self, DocxError> {
        let package = Package::open(bytes)?;
        let document = package.required(DOCUMENT_PART)?;
        let xml = String::from_utf8(document)
            .map_err(|_| DocxError::NotUtf8(DOCUMENT_PART.to_owned()))?;
        let scanned = parse::scan(&xml)?;
        let rels = Relationships::parse(&package.required(DOCUMENT_RELS_PART).unwrap_or_default())?;
        let content_types =
            ContentTypes::parse(&package.required(CONTENT_TYPES_PART).unwrap_or_default())?;
        Ok(Self {
            package,
            xml,
            blocks: scanned.blocks,
            body_close: scanned.body_close,
            rels,
            content_types,
            dirty_xml: false,
            dirty_rels: false,
            dirty_content_types: false,
            next_generated_id: 1000,
        })
    }

    /// The scanned blocks in document order: every body paragraph, every table, and every
    /// paragraph inside a table cell, in the order they appear.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// The paragraphs, with their block numbers, in document order.
    pub fn paragraphs(&self) -> impl Iterator<Item = (usize, &Paragraph)> {
        self.blocks.iter().enumerate().filter_map(|(index, block)| match block {
            Block::Paragraph(paragraph) => Some((index, paragraph)),
            Block::Table(_) => None,
        })
    }

    /// The main part's XML, as edited so far.
    pub fn document_xml(&self) -> &str {
        &self.xml
    }

    /// The bytes of a container part, decompressed.
    pub fn part(&self, name: &str) -> Option<Result<Vec<u8>, DocxError>> {
        self.package.part(name)
    }

    /// The names of the container's parts, in archive order.
    pub fn part_names(&self) -> impl Iterator<Item = &str> {
        self.package.part_names()
    }

    /// Adds or replaces a container part outright. The index is untouched; call this for parts
    /// this crate has no view of.
    pub fn set_part(&mut self, name: &str, data: Vec<u8>) {
        self.package.set_part(name, data);
    }

    /// Resizes an object's preview shape to `width_pt` × `height_pt`, so a regenerated preview
    /// picture is shown at its natural size instead of being stretched into the shape the old
    /// picture was drawn for. The `v:shape` style and the enclosing `w:object`'s
    /// `dxaOrig`/`dyaOrig` (twips, 20 per point) are both rewritten.
    pub fn resize_object_preview(
        &mut self,
        block: usize,
        item: usize,
        width_pt: f64,
        height_pt: f64,
    ) -> Result<(), DocxError> {
        let (_, object_range) = {
            let paragraph = self.paragraph_at(block, item)?;
            match &paragraph.items[item] {
                Item::Object { range, preview_range: Some(preview), .. } => {
                    (preview.clone(), range.clone())
                }
                _ => return Err(DocxError::Xml("the item has no preview shape to resize".to_owned())),
            }
        };
        let object = self.xml[object_range.clone()].to_owned();
        let resized_object = rewrite_object_extent(&object, width_pt, height_pt);
        // Each splice rescans the index, so the shape is re-located after the object edit.
        self.splice(object_range, &resized_object)?;
        let shape_range = {
            let paragraph = self.paragraph_at(block, item)?;
            match &paragraph.items[item] {
                Item::Object { preview_range: Some(preview), .. } => preview.clone(),
                _ => return Err(DocxError::Xml("the preview shape vanished".to_owned())),
            }
        };
        let shape = self.xml[shape_range.clone()].to_owned();
        let resized = rewrite_shape_extent(&shape, width_pt, height_pt)?;
        self.splice(shape_range, &resized)?;
        Ok(())
    }

    /// Every formula in the document, in document order, with its tree when the carrier's bytes
    /// could be read into one.
    pub fn equations(&self) -> Vec<Equation> {
        let mut found = Vec::new();
        for (block_index, block) in self.blocks.iter().enumerate() {
            let Block::Paragraph(paragraph) = block else { continue };
            for (item_index, item) in paragraph.items.iter().enumerate() {
                match item {
                    Item::OMath { range, display } => found.push(Equation {
                        block: block_index,
                        item: item_index,
                        kind: EquationKind::Omath { display: *display },
                        math: mt_omml::parse(&self.xml[range.clone()]).ok(),
                    }),
                    Item::Object { data_rel_id, prog_id, .. } => found.push(Equation {
                        block: block_index,
                        item: item_index,
                        kind: EquationKind::Ole {
                            prog_id: prog_id.clone(),
                            part: data_rel_id.as_deref().and_then(|id| self.relationship_part(id)),
                        },
                        math: self.object_tree(data_rel_id),
                    }),
                    _ => {}
                }
            }
        }
        found
    }

    /// Replaces one item with new content and rescans the index.
    ///
    /// The coordinates stay those of the most recent scan; enumerate again after an edit.
    pub fn replace(
        &mut self,
        block: usize,
        item: usize,
        with: Replacement,
    ) -> Result<(), DocxError> {
        // Everything the edit needs from the target is copied out first: the helpers below take
        // the document by mutable reference.
        let (range, target) = self.replace_target(block, item)?;
        let keep_object_part = matches!(with, Replacement::Ole { .. });
        let fragment = match with {
            Replacement::OMath { math, display } => fragment::o_math(&math, display),
            Replacement::Text { text, style } => fragment::text_run(&text, &style),
            Replacement::Image(data) => {
                let rel_id = self.add_media_part(&data)?;
                let id = self.take_generated_id();
                fragment::image_run(&rel_id, &data.name, data.extent_emu, id)
            }
            Replacement::Ole { math, prog_id, preview } => {
                let (rel_id, prog_id, preview) =
                    self.ole_replacement(&target, &math, prog_id.as_deref(), preview.as_ref())?;
                let id = self.take_generated_id();
                fragment::object_run(&rel_id, &prog_id, preview.as_deref(), id)
            }
            Replacement::Raw(xml) => xml,
        };
        if !keep_object_part {
            if let Some(rel_id) = &target.rel_id {
                self.retire_object(rel_id, block, item);
            }
        }
        self.splice(range, &fragment)
    }

    /// The range an edit cuts and the object facts an edit may reuse, for one item.
    fn replace_target(&self, block: usize, item: usize) -> Result<(Range<usize>, ObjectTarget), DocxError> {
        let paragraph = self.paragraph_at(block, item)?;
        let target = &paragraph.items[item];
        let object = match target {
            Item::Object { data_rel_id, prog_id, preview_range, .. } => ObjectTarget {
                rel_id: data_rel_id.clone(),
                prog_id: prog_id.clone(),
                preview_range: preview_range.clone(),
            },
            _ => ObjectTarget::default(),
        };
        Ok((target.edit_range(), object))
    }

    /// Inserts an Office Math formula.
    pub fn insert_omath(
        &mut self,
        position: Position,
        math: &Math,
        display: bool,
    ) -> Result<(), DocxError> {
        let fragment = fragment::o_math(math, display);
        let fragment = match position {
            // At the end of the body the fragment must be a whole paragraph.
            Position::BodyEnd => fragment::paragraph(&fragment),
            _ => fragment,
        };
        self.insert_fragment(position, &fragment)
    }

    /// Inserts field markup — a `w:fldSimple` element or an `instrText` run — given as raw XML.
    ///
    /// The XML is spliced in verbatim; the caller owns well-formedness and namespace bindings.
    /// At [`Position::BodyEnd`] it must be a whole paragraph, elsewhere run-level markup.
    pub fn insert_field(&mut self, position: Position, field_xml: &str) -> Result<(), DocxError> {
        self.insert_fragment(position, field_xml)
    }

    /// Inserts raw XML at a position, verbatim.
    pub fn insert_fragment(&mut self, position: Position, xml: &str) -> Result<(), DocxError> {
        let offset = match position {
            Position::BodyEnd => self.body_close,
            Position::ParagraphEnd { block } => self.paragraph_at_block(block)?.content_end,
            Position::ParagraphPropertiesStart { block } => {
                let paragraph = self.paragraph_at_block(block)?;
                paragraph.properties_open_end.unwrap_or(paragraph.open_tag_end)
            }
            Position::AfterParagraph { block } => {
                let paragraph = self.paragraph_at_block(block)?;
                paragraph.range.end
            }
            Position::BeforeItem { block, item } | Position::AfterItem { block, item } => {
                let paragraph = self.paragraph_at(block, item)?;
                let range = paragraph.items[item].edit_range();
                match position {
                    Position::BeforeItem { .. } => range.start,
                    _ => range.end,
                }
            }
        };
        self.splice_at(offset, xml)
    }

    /// Cuts a range out of the main part's XML and puts the fragment in its place, rescanning
    /// the index.
    ///
    /// The range is an offset range into [`Document::document_xml`], valid until the next edit;
    /// callers that located markup in the XML should splice it in at once. Well-formedness is
    /// the caller's, as with [`Document::insert_field`].
    pub fn splice_range(&mut self, range: Range<usize>, xml: &str) -> Result<(), DocxError> {
        self.splice(range, xml)
    }

    /// Writes the container: edited parts fresh, everything untouched byte-for-byte.
    pub fn into_bytes(mut self) -> Result<Vec<u8>, DocxError> {
        if self.dirty_rels {
            let xml = self.rels.to_xml();
            self.package.set_part(DOCUMENT_RELS_PART, xml.into_bytes());
        }
        if self.dirty_content_types {
            let xml = self.content_types.to_xml();
            self.package.set_part(CONTENT_TYPES_PART, xml.into_bytes());
        }
        if self.dirty_xml {
            self.package.set_part(DOCUMENT_PART, self.xml.clone().into_bytes());
        }
        self.package.into_bytes()
    }

    // ------------------------------------------------------------------ internals

    /// The paragraph that owns this item.
    fn paragraph_at(&self, block: usize, item: usize) -> Result<&Paragraph, DocxError> {
        let paragraph = self.paragraph_at_block(block)?;
        paragraph.items.get(item).ok_or_else(|| {
            DocxError::Xml(format!("paragraph {block} has no item {item}"))
        })?;
        Ok(paragraph)
    }

    fn paragraph_at_block(&self, block: usize) -> Result<&Paragraph, DocxError> {
        match self.blocks.get(block) {
            Some(Block::Paragraph(paragraph)) => Ok(paragraph),
            Some(Block::Table(_)) => {
                Err(DocxError::Xml(format!("block {block} is a table, not a paragraph")))
            }
            None => Err(DocxError::Xml(format!("the document has no block {block}"))),
        }
    }

    /// The part an `r:id` of the main part points at, if it is internal to the container.
    ///
    /// Exporters use this to reach the bytes a picture or object relationship names.
    pub fn relationship_part(&self, id: &str) -> Option<String> {
        let rel = self.rels.by_id(id)?;
        (!rel.external).then(|| rels::resolve("word", &rel.target))
    }

    /// The equation tree inside the object a relationship points at, if there is one.
    fn object_tree(&self, data_rel_id: &Option<String>) -> Option<Math> {
        let part = self.relationship_part(data_rel_id.as_deref()?)?;
        let bytes = self.part(&part).and_then(|result| result.ok())?;
        let stream = mt_mtef::read_from_ole(&bytes).ok()??;
        mt_mtef::parse(&stream).ok()
    }

    /// Cuts a range out of the main part's XML and puts the fragment in its place.
    fn splice(&mut self, range: std::ops::Range<usize>, fragment: &str) -> Result<(), DocxError> {
        self.xml.replace_range(range, fragment);
        self.rescan()
    }

    /// Puts the fragment in at an offset, moving what follows it.
    fn splice_at(&mut self, offset: usize, fragment: &str) -> Result<(), DocxError> {
        self.xml.insert_str(offset, fragment);
        self.rescan()
    }

    fn rescan(&mut self) -> Result<(), DocxError> {
        let scanned = parse::scan(&self.xml)?;
        self.blocks = scanned.blocks;
        self.body_close = scanned.body_close;
        self.dirty_xml = true;
        Ok(())
    }

    fn take_generated_id(&mut self) -> u32 {
        let id = self.next_generated_id;
        self.next_generated_id += 1;
        id
    }

    /// Adds a media part and its relationship; returns the relationship id.
    fn add_media_part(&mut self, data: &ImageData) -> Result<String, DocxError> {
        let part = self.unused_part_name("word/media", "image", &data.extension);
        let target = relationship_target(&part);
        if self.content_types.ensure_default(&data.extension, &data.content_type) {
            self.dirty_content_types = true;
        }
        self.package.set_part(&part, data.bytes.clone());
        let rel_id = self.rels.add(kinds::IMAGE, &target, false);
        self.dirty_rels = true;
        Ok(rel_id)
    }

    /// Rewrites or creates the compound file behind an equation object; returns the relationship
    /// id, the ProgID, and the preview markup to put back.
    fn ole_replacement(
        &mut self,
        target: &ObjectTarget,
        math: &Math,
        wanted_prog_id: Option<&str>,
        wanted_preview: Option<&ImageData>,
    ) -> Result<(String, String, Option<String>), DocxError> {
        let stream = mt_mtef::write(math);
        if let Some(rel_id) = &target.rel_id {
            if let Some(part) = self.relationship_part(rel_id) {
                if let Some(Ok(existing)) = self.package.part(&part) {
                    if let Ok(rewritten) = mt_mtef::write_into_ole(&existing, &stream) {
                        self.package.set_part(&part, rewritten);
                        let preview = target
                            .preview_range
                            .clone()
                            .map(|range| self.xml[range].to_owned());
                        let prog_id =
                            target.prog_id.clone().unwrap_or_else(|| DEFAULT_PROG_ID.to_owned());
                        return Ok((rel_id.clone(), prog_id, preview));
                    }
                }
            }
        }
        // No usable object behind the item: a new minimal one.
        self.create_equation_object(math, wanted_prog_id.unwrap_or(DEFAULT_PROG_ID), wanted_preview)
    }

    /// Creates a fresh equation object — a compound-file part holding `math`, an optional
    /// preview picture part — and returns the relationship id, the ProgID, and the preview
    /// markup to embed.
    fn create_equation_object(
        &mut self,
        math: &Math,
        prog_id: &str,
        preview: Option<&ImageData>,
    ) -> Result<(String, String, Option<String>), DocxError> {
        let stream = mt_mtef::write(math);
        let bytes = mt_mtef::create_ole(&stream)
            .map_err(|error| DocxError::Xml(format!("building an object failed: {error}")))?;
        let part = self.unused_part_name("word/embeddings", "oleObject", "bin");
        let rel_target = relationship_target(&part);
        if self.content_types.ensure_default("bin", OLE_CONTENT_TYPE) {
            self.dirty_content_types = true;
        }
        self.package.set_part(&part, bytes);
        let rel_id = self.rels.add(kinds::OLE_OBJECT, &rel_target, false);
        self.dirty_rels = true;
        let preview = match preview {
            Some(picture) => {
                let image_rel = self.add_media_part(picture)?;
                let id = self.take_generated_id();
                Some(fragment::object_preview(&image_rel, picture.extent_emu, id))
            }
            None => None,
        };
        Ok((rel_id, prog_id.to_owned(), preview))
    }

    /// The run markup embedding a fresh equation object, ready to splice into any paragraph:
    /// the object's compound file (holding `math`) and optional preview picture become parts of
    /// the container, referenced by the returned `w:r`.
    pub fn new_equation_object_runs(
        &mut self,
        math: &Math,
        prog_id: &str,
        preview: Option<&ImageData>,
    ) -> Result<String, DocxError> {
        let (rel_id, _prog_id, preview) = self.create_equation_object(math, prog_id, preview)?;
        let id = self.take_generated_id();
        Ok(fragment::object_run(&rel_id, prog_id, preview.as_deref(), id))
    }

    /// Drops the relationship of an object that is being replaced by something else, when no
    /// other object shares it. The part itself is left in the container: other parts' own
    /// relationships may still point at it.
    fn retire_object(&mut self, rel_id: &str, block: usize, item: usize) {
        let still_used = self.blocks.iter().enumerate().any(|(block_index, other)| {
            let Block::Paragraph(paragraph) = other else { return false };
            paragraph.items.iter().enumerate().any(|(item_index, entry)| {
                (block_index != block || item_index != item)
                    && matches!(entry, Item::Object { data_rel_id: Some(id), .. } if id == rel_id)
            })
        });
        if !still_used {
            self.rels.remove(rel_id);
            self.dirty_rels = true;
        }
    }

    /// The first `word/dir/stemN.ext` no existing part uses.
    fn unused_part_name(&self, directory: &str, stem: &str, extension: &str) -> String {
        let mut number = 1;
        loop {
            let candidate = format!("{directory}/{stem}{number}.{extension}");
            if self.package.part(&candidate).is_none() {
                return candidate;
            }
            number += 1;
        }
    }
}

/// The relationship target of a part: its name relative to the main part's directory.
fn relationship_target(part: &str) -> String {
    part.strip_prefix("word/").unwrap_or(part).to_owned()
}

/// The object facts a replacement may reuse, copied out of the index before an edit begins.
#[derive(Default)]
struct ObjectTarget {
    rel_id: Option<String>,
    prog_id: Option<String>,
    preview_range: Option<Range<usize>>,
}


/// Rewrites a `v:shape`'s `style` width/height (points) to the given natural size.
fn rewrite_shape_extent(shape: &str, width_pt: f64, height_pt: f64) -> Result<String, DocxError> {
    let replacement = format!("width:{width_pt:.1}pt;height:{height_pt:.1}pt");
    let mut out = String::with_capacity(shape.len());
    let mut rest = shape;
    let mut replaced = false;
    while let Some(at) = rest.find("width:") {
        let after = &rest[at..];
        let end = after.find(';').map_or(after.len(), |i| i);
        out.push_str(&rest[..at]);
        out.push_str(&replacement);
        rest = &after[end..];
        replaced = true;
    }
    if !replaced {
        return Err(DocxError::Xml("the preview shape has no size style to rewrite".to_owned()));
    }
    out.push_str(rest);
    Ok(out)
}

/// Rewrites a `w:object`'s `dxaOrig`/`dyaOrig` (twips) to the given natural size.
fn rewrite_object_extent(object: &str, width_pt: f64, height_pt: f64) -> String {
    let dxa = (width_pt * 20.0).round() as u32;
    let dya = (height_pt * 20.0).round() as u32;
    let object = replace_attribute_number(object, "dxaOrig", &dxa.to_string());
    replace_attribute_number(&object, "dyaOrig", &dya.to_string())
}

/// Replaces the number inside `name="<digits>"` (first occurrence) with `value`.
fn replace_attribute_number(text: &str, name: &str, value: &str) -> String {
    let opener = format!("{name}=\"");
    let Some(at) = text.find(&opener) else { return text.to_owned() };
    let digits_start = at + opener.len();
    let digits_end = text[digits_start..]
        .find('"')
        .map_or(text.len(), |i| digits_start + i);
    let mut out = String::with_capacity(text.len() + value.len());
    out.push_str(&text[..digits_start]);
    out.push_str(value);
    out.push_str(&text[digits_end..]);
    out
}
