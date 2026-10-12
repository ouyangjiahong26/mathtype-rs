//! The document index: paragraphs and the things inside them, each carrying the byte range of
//! its original XML.
//!
//! Nothing here rebuilds a document. Every item remembers where its markup sits in
//! `word/document.xml`, so an editor can cut out, splice in, or read back the exact bytes, and an
//! exporter can slice out structure it wants to map rather than re-parse. The only things with a
//! shape of their own are the ones the equation tools act on: text runs, Office Math, embedded
//! objects, and images. Everything else — headings, numbering, tables, fields, hyperlinks —
//! travels as an opaque range.

use std::ops::Range;

use mt_core::Math;

/// Vertical alignment of a run's characters, the `w:vertAlign` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertAlign {
    /// Raised, the exponent position.
    Superscript,
    /// Lowered, the index position.
    Subscript,
}

/// The style facts of a text run this crate can read out of `w:rPr` and write back.
///
/// Everything else a run's properties may carry stays in the original bytes and survives edits
/// untouched, because a styled replacement run is only generated when the caller asks for one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunStyle {
    /// `w:b` — bold on when `Some(true)`, explicitly off when `Some(false)`.
    pub bold: Option<bool>,
    /// `w:i` — italic on when `Some(true)`, explicitly off when `Some(false)`.
    pub italic: Option<bool>,
    /// `w:vertAlign` — super- or subscript.
    pub vert_align: Option<VertAlign>,
    /// `w:sz` — half points, 24 meaning 12pt.
    pub size_half_points: Option<u32>,
}

/// One entry in a paragraph's item sequence.
#[derive(Clone, Debug)]
pub enum Item {
    /// A text run: plain characters with optional styling.
    Text {
        /// The whole `w:r` element.
        range: Range<usize>,
        /// The concatenated characters of the run's `w:t` elements, entities resolved.
        text: String,
        /// The style summary of `w:rPr`.
        style: RunStyle,
    },
    /// An Office Math formula, inline (`m:oMath`) or displayed (`m:oMathPara`).
    OMath {
        /// The whole `m:oMath` or `m:oMathPara` element.
        range: Range<usize>,
        /// Whether this is the display form, `m:oMathPara`.
        display: bool,
    },
    /// An embedded object: an OLE compound file with an equation stream, shown by a picture.
    Object {
        /// The whole `w:r` element wrapping the object.
        run_range: Range<usize>,
        /// The `w:object` element inside the run.
        range: Range<usize>,
        /// The part of `w:object`'s content before `o:OLEObject`: the shape type and preview
        /// picture markup, kept so a replacement can put it back verbatim.
        preview_range: Option<Range<usize>>,
        /// `ProgID`, `Equation.3` for the equation editor's objects.
        prog_id: Option<String>,
        /// The relationship id of the data part, `r:id` on `o:OLEObject`.
        data_rel_id: Option<String>,
        /// The relationship id of the preview image, `r:id` on `v:imagedata`.
        preview_rel_id: Option<String>,
    },
    /// A picture: a `w:drawing` or `w:pict` inside a run.
    Image {
        /// The whole `w:r` element wrapping the picture.
        run_range: Range<usize>,
        /// The `w:drawing` or `w:pict` element inside the run.
        range: Range<usize>,
        /// The relationship id the picture's bytes hang from, `r:embed` or `r:id`.
        rel_id: Option<String>,
        /// The picture's name, `wp:docPr@name` when there is one.
        name: Option<String>,
    },
    /// Anything else in the paragraph, kept as its original bytes: properties, bookmarks,
    /// fields, hyperlinks, stray markup.
    Other {
        /// The bytes of this stretch of the paragraph.
        range: Range<usize>,
    },
}

impl Item {
    /// The range an edit replaces: the element itself for formulas, the run around the rest.
    pub fn edit_range(&self) -> Range<usize> {
        match self {
            Item::Text { range, .. }
            | Item::Object {
                run_range: range, ..
            }
            | Item::Image {
                run_range: range, ..
            }
            | Item::Other { range } => range.clone(),
            Item::OMath { range, .. } => range.clone(),
        }
    }

    /// What kind of thing this item is, for quick filtering.
    pub fn kind(&self) -> &'static str {
        match self {
            Item::Text { .. } => "text",
            Item::OMath { .. } => "omath",
            Item::Object { .. } => "object",
            Item::Image { .. } => "image",
            Item::Other { .. } => "other",
        }
    }
}

/// A paragraph, with the items the index recognises and everything else folded into `Other`.
#[derive(Clone, Debug)]
pub struct Paragraph {
    /// The whole `w:p` element.
    pub range: Range<usize>,
    /// The `w:pStyle` name, `Heading1` for instance.
    pub style: Option<String>,
    /// Whether the paragraph carries `w:numPr`, so it is part of a list.
    pub numbered: bool,
    /// How many tables deep the paragraph sits; zero for body paragraphs.
    pub table_depth: usize,
    /// Where the paragraph's opening tag ends — the insertion point for a new `w:pPr`, which
    /// must be the paragraph's first child.
    pub open_tag_end: usize,
    /// Where the paragraph's `w:pPr` opening tag ends, when it has properties: the insertion
    /// point for new property elements. `None` when the paragraph has no properties.
    pub properties_open_end: Option<usize>,
    /// Where the paragraph's content ends — the offset its closing tag starts at, the insertion
    /// point for markup appended to the paragraph.
    pub content_end: usize,
    /// The paragraph's contents in document order.
    pub items: Vec<Item>,
}

impl Paragraph {
    /// The paragraph's visible characters, from its text runs only.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for item in &self.items {
            if let Item::Text { text, .. } = item {
                out.push_str(text);
            }
        }
        out
    }

    /// Whether the paragraph holds at least one formula or embedded object.
    pub fn has_equation(&self) -> bool {
        self.items
            .iter()
            .any(|item| matches!(item, Item::OMath { .. } | Item::Object { .. }))
    }
}

/// A table, carried whole: the index records where it sits and how large it is, and an exporter
/// slices out the bytes when it wants to map one. The paragraphs inside a table are indexed as
/// paragraphs of their own, so formulas in table cells are found like any others.
#[derive(Clone, Debug)]
pub struct Table {
    /// The whole `w:tbl` element.
    pub range: Range<usize>,
    /// The row count.
    pub rows: usize,
    /// The column count, from the grid when the table declares one, else the widest row.
    pub columns: usize,
}

/// A body-level block: a paragraph or a table, in document order.
#[derive(Clone, Debug)]
pub enum Block {
    /// A paragraph, inside or outside a table.
    Paragraph(Paragraph),
    /// A table.
    Table(Table),
}

/// A located formula, whatever carries it.
#[derive(Clone, Debug)]
pub struct Equation {
    /// The block the formula sits in.
    pub block: usize,
    /// The item inside that block.
    pub item: usize,
    /// Which kind of carrier and what hangs from it.
    pub kind: EquationKind,
    /// The formula as a tree, when the carrier's bytes could be read into one.
    pub math: Option<Math>,
}

/// What carries a located formula.
#[derive(Clone, Debug)]
pub enum EquationKind {
    /// Office Math markup.
    Omath {
        /// Whether this is the display form, `m:oMathPara`.
        display: bool,
    },
    /// An embedded OLE object.
    Ole {
        /// `ProgID`, `Equation.3` for instance.
        prog_id: Option<String>,
        /// The data part, `word/embeddings/oleObject1.bin` for instance.
        part: Option<String>,
    },
}
