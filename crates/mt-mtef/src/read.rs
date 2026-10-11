//! Reading an equation stream into the equation tree.
//!
//! The reader is driven by the record tags: it never assumes how long a record is, so a stream
//! that uses records this crate does not map to the tree still reads on to the end.

use mt_core::{
    Accent, BigOp, Class, Delim, Fence, Font, Frac, FracKind, Limits, Math, MathStyle, Matrix,
    MatrixKind, Node, Row, Scripts, Side, Sqrt, Space,
};

use crate::tables::*;
use crate::{Dimension, FontDef, Header, MtefError, Parsed, Preferences, Report, StyleDef, Unit};

/// Object-list nesting the reader accepts before it gives up on the stream.
const MAX_DEPTH: usize = 48;

/// Records the reader accepts in a chain that wraps the previous node without recursing.
///
/// Scripts and embellishments each nest the node they decorate by one level while the reader
/// loops instead of recursing, so the depth gate never sees them. Bounding the chain keeps the
/// resulting tree shallow enough for the recursive walks that follow (normalize, walk, drop).
const MAX_CHAIN: usize = 512;

/// Read a bare equation byte stream, i.e. one that starts at the version byte.
pub(crate) fn document(body: &[u8]) -> Result<Parsed, MtefError> {
    let mut parser = Parser::new(body)?;
    let nodes = parser.list()?;
    let mut math = Math { body: Node::Row(nodes), style: parser.style };
    math.body.normalize();
    Ok(Parsed { math, header: parser.header, report: parser.report })
}

// ------------------------------------------------------------------------------- reader

/// Bounds-checked cursor over the stream.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn peek(&self) -> Result<u8, MtefError> {
        self.data.get(self.pos).copied().ok_or(MtefError::Truncated)
    }

    fn u8(&mut self) -> Result<u8, MtefError> {
        let value = self.peek()?;
        self.pos += 1;
        Ok(value)
    }

    fn bytes(&mut self, count: usize) -> Result<&'a [u8], MtefError> {
        let end = self.pos.checked_add(count).ok_or(MtefError::Truncated)?;
        let slice = self.data.get(self.pos..end).ok_or(MtefError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    fn u16(&mut self) -> Result<u16, MtefError> {
        let raw = self.bytes(2)?;
        Ok(u16::from_le_bytes([raw[0], raw[1]]))
    }

    /// An unsigned value: one byte, or `0xFF` followed by a sixteen-bit value.
    fn unsigned(&mut self) -> Result<usize, MtefError> {
        let first = self.u8()?;
        if first < 0xFF {
            Ok(usize::from(first))
        } else {
            Ok(usize::from(self.u16()?))
        }
    }

    /// A signed value biased by 128, or `0xFF` followed by a sixteen-bit value biased by 32768.
    fn signed(&mut self) -> Result<i32, MtefError> {
        let first = self.u8()?;
        if first == 0xFF {
            Ok(i32::from(self.u16()?) - 32768)
        } else {
            Ok(i32::from(first) - 128)
        }
    }

    /// A nudge offset: two bytes, or a marker followed by two sixteen-bit offsets.
    fn nudge(&mut self) -> Result<(), MtefError> {
        let dx = self.u8()?;
        let dy = self.u8()?;
        if dx == 128 && dy == 128 {
            self.bytes(4)?;
        }
        Ok(())
    }

    /// A NUL-terminated string, whose bytes are read as characters one for one. The strings in
    /// a stream are font and encoding names, which are ascii in every stream this crate writes.
    fn cstring(&mut self) -> Result<String, MtefError> {
        let tail = self.data.get(self.pos..).ok_or(MtefError::Truncated)?;
        let length = tail.iter().position(|&byte| byte == 0).ok_or(MtefError::Truncated)?;
        let text = tail[..length].iter().map(|&byte| char::from(byte)).collect();
        self.pos += length + 1;
        Ok(text)
    }
}

/// Nibble stream of a dimension array.
struct Nibbles<'a> {
    data: &'a [u8],
    pos: usize,
    high: bool,
}

impl<'a> Nibbles<'a> {
    fn next(&mut self) -> Result<u8, MtefError> {
        let byte = *self.data.get(self.pos).ok_or(MtefError::Truncated)?;
        let nibble = if self.high { byte & 0x0F } else { byte >> 4 };
        if self.high {
            self.pos += 1;
        }
        self.high = !self.high;
        Ok(nibble)
    }

    /// Value string of one dimension: digits until the terminating nibble.
    fn dimension(&mut self, unit: u8) -> Result<Dimension, MtefError> {
        let mut value = String::new();
        loop {
            match self.next()? {
                0x0F => break,
                digit @ 0x00..=0x09 => value.push(char::from(b'0' + digit)),
                0x0A => value.push('.'),
                0x0B => value.push('-'),
                _ => value.push('?'),
            }
        }
        Ok(Dimension { unit: Unit::from_code(unit), value })
    }
}

// ------------------------------------------------------------------------------- parser

/// One item of an object list.
enum Item {
    Nodes(Vec<Node>),
    /// A line, kept whole when the caller reads a pile or a table. `null` marks a line that is
    /// not drawn at all, which an optional slot reads as absent — a line that is drawn but
    /// empty is a slot that is present and empty.
    Line { row: Row, null: bool },
    /// A script template, whose base is the record next to it.
    Script { sub: Option<Node>, sup: Option<Node>, precedes: bool },
}

struct Parser<'a> {
    reader: Reader<'a>,
    header: Header,
    /// Font style definitions, in the order they were read.
    font_styles: Vec<(usize, u8)>,
    report: Report,
    style: MathStyle,
    /// Whether a size record has already set the style of the equation.
    style_set: bool,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn new(data: &'a [u8]) -> Result<Self, MtefError> {
        let mut parser = Self {
            reader: Reader::new(data),
            header: Header::default(),
            font_styles: Vec::new(),
            report: Report::default(),
            style: MathStyle::Full,
            style_set: false,
            depth: 0,
        };
        parser.stream_header()?;
        Ok(parser)
    }

    /// Read the stream header: version, producer and application key.
    fn stream_header(&mut self) -> Result<(), MtefError> {
        let version = self.reader.u8()?;
        if version != 5 {
            return Err(MtefError::Version(version));
        }
        self.header.version = version;
        self.header.platform = self.reader.u8()?;
        self.header.product = self.reader.u8()?;
        self.header.product_version = self.reader.u8()?;
        self.header.product_subversion = self.reader.u8()?;
        self.header.creator = self.reader.cstring()?;
        self.header.options = self.reader.u8()?;
        Ok(())
    }

    // ------------------------------------------------------------------ lists

    /// Read records until the next `END`, flattening lines into the surrounding row.
    fn list(&mut self) -> Result<Vec<Node>, MtefError> {
        self.enter()?;
        let result = self.list_inner();
        self.leave();
        result
    }

    fn list_inner(&mut self) -> Result<Vec<Node>, MtefError> {
        let mut out: Vec<Node> = Vec::new();
        let mut pending: Option<(Option<Node>, Option<Node>)> = None;
        // A run of scripts wraps the same node over and over, which the depth gate does not see.
        let mut chain = 0usize;
        let push = |out: &mut Vec<Node>, node: Node, pending: &mut Option<_>| {
            out.push(match pending.take() {
                Some((sub, sup)) => attach(node, sub, sup),
                None => node,
            });
        };
        loop {
            if self.reader.peek()? == TAG_END {
                self.reader.u8()?;
                self.report.records += 1;
                return Ok(out);
            }
            match self.item(false)? {
                Item::Nodes(nodes) => {
                    chain = 0;
                    for node in nodes {
                        push(&mut out, node, &mut pending);
                    }
                }
                Item::Line { row, .. } => {
                    chain = 0;
                    for node in row {
                        push(&mut out, node, &mut pending);
                    }
                }
                Item::Script { sub, sup, precedes: false } => {
                    chain += 1;
                    if chain > MAX_CHAIN {
                        return Err(MtefError::Nesting);
                    }
                    let base = out.pop().unwrap_or_else(Node::empty);
                    out.push(attach(base, sub, sup));
                }
                Item::Script { sub, sup, precedes: true } => pending = Some((sub, sup)),
            }
        }
    }

    /// Read the object list of a template whole, keeping its lines apart.
    fn items(&mut self) -> Result<Vec<Item>, MtefError> {
        self.enter()?;
        let result = self.items_inner();
        self.leave();
        result
    }

    fn items_inner(&mut self) -> Result<Vec<Item>, MtefError> {
        let mut out = Vec::new();
        loop {
            if self.reader.peek()? == TAG_END {
                self.reader.u8()?;
                self.report.records += 1;
                return Ok(out);
            }
            out.push(self.item(true)?);
        }
    }

    fn enter(&mut self) -> Result<(), MtefError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(MtefError::Nesting);
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    // --------------------------------------------------------------- records

    fn item(&mut self, lines: bool) -> Result<Item, MtefError> {
        let tag = self.reader.u8()?;
        self.report.records += 1;
        match tag {
            TAG_LINE => self.line(lines),
            TAG_CHAR => self.character(),
            TAG_TMPL => self.template(),
            TAG_PILE => self.pile(),
            TAG_MATRIX => self.matrix(),
            TAG_EMBELL => {
                // An embellishment that does not follow a character decorates nothing.
                self.embellishment_type()?;
                self.report.skipped += 1;
                Ok(Item::Nodes(Vec::new()))
            }
            TAG_RULER => {
                self.ruler()?;
                self.report.skipped += 1;
                Ok(Item::Nodes(Vec::new()))
            }
            TAG_FONT_STYLE_DEF => {
                let font = self.reader.unsigned()?;
                let style = self.reader.u8()?;
                self.font_styles.push((font, style));
                Ok(Item::Nodes(Vec::new()))
            }
            TAG_SIZE | TAG_TYPESIZE_FIRST..=TAG_TYPESIZE_LAST => {
                let style = self.size(tag)?;
                if self.style_set {
                    self.report.skipped += 1;
                } else {
                    self.style = style;
                    self.style_set = true;
                }
                Ok(Item::Nodes(Vec::new()))
            }
            TAG_COLOR => {
                self.reader.unsigned()?;
                self.report.skipped += 1;
                Ok(Item::Nodes(Vec::new()))
            }
            TAG_COLOR_DEF => {
                self.color_definition()?;
                self.report.skipped += 1;
                Ok(Item::Nodes(Vec::new()))
            }
            TAG_FONT_DEF => {
                let encoding = self.reader.unsigned()?;
                let name = self.reader.cstring()?;
                self.header.fonts.push(FontDef { encoding, name });
                Ok(Item::Nodes(Vec::new()))
            }
            TAG_EQN_PREFS => {
                self.equation_preferences()?;
                Ok(Item::Nodes(Vec::new()))
            }
            TAG_ENCODING_DEF => {
                let name = self.reader.cstring()?;
                self.header.encodings.push(name);
                Ok(Item::Nodes(Vec::new()))
            }
            tag if tag >= TAG_FUTURE => {
                // A record for a future version carries its own length.
                let length = self.reader.unsigned()?;
                self.reader.bytes(length)?;
                self.report.skipped += 1;
                Ok(Item::Nodes(Vec::new()))
            }
            other => Err(MtefError::UnknownRecord(other)),
        }
    }

    fn line(&mut self, lines: bool) -> Result<Item, MtefError> {
        let options = self.reader.u8()?;
        if options & OPT_NUDGE != 0 {
            self.reader.nudge()?;
        }
        if options & OPT_LINE_LSPACE != 0 {
            self.reader.u16()?;
        }
        if options & OPT_LP_RULER != 0 {
            self.enter()?;
            let result = self.item(true);
            self.leave();
            result?;
        }
        if options & OPT_LINE_NULL != 0 {
            return Ok(if lines {
                Item::Line { row: Row::new(), null: true }
            } else {
                Item::Nodes(Vec::new())
            });
        }
        let contents = self.list()?;
        Ok(if lines { Item::Line { row: contents, null: false } } else { Item::Nodes(contents) })
    }

    fn character(&mut self) -> Result<Item, MtefError> {
        let options = self.reader.u8()?;
        if options & OPT_NUDGE != 0 {
            self.reader.nudge()?;
        }
        let typeface = self.reader.signed()?;
        let code = if options & OPT_CHAR_NO_MTCODE == 0 { Some(self.reader.u16()?) } else { None };
        let eight = if options & OPT_CHAR_8BIT != 0 { Some(self.reader.u8()?) } else { None };
        let sixteen = if options & OPT_CHAR_16BIT != 0 { Some(self.reader.u16()?) } else { None };
        let ch = code
            .and_then(|value| char::from_u32(u32::from(value)))
            .or_else(|| sixteen.and_then(|value| char::from_u32(u32::from(value))))
            .or_else(|| eight.map(char::from))
            .unwrap_or('\u{FFFD}');
        let mut node = if typeface == TF_SPACE {
            Node::Space(Space::Thin)
        } else {
            Node::Char(mt_core::Char {
                ch,
                class: class_of(ch, typeface),
                font: self.font_of(typeface),
            })
        };
        if options & OPT_CHAR_EMBELL != 0 {
            node = self.embellishments(node)?;
        }
        Ok(Item::Nodes(vec![node]))
    }

    /// Read the embellishment list of a character and dress the character in it.
    fn embellishments(&mut self, node: Node) -> Result<Node, MtefError> {
        self.enter()?;
        let mut dressed = node;
        // Each embellishment adds one accent around the same base, which the depth gate does not
        // see: bound the chain so the tree stays shallow.
        let mut chain = 0usize;
        let result = loop {
            if self.reader.peek()? == TAG_END {
                self.reader.u8()?;
                self.report.records += 1;
                break Ok(dressed);
            }
            if self.reader.peek()? == TAG_EMBELL {
                self.reader.u8()?;
                self.report.records += 1;
                let kind = self.embellishment_type()?;
                match accent_of_embellishment(kind) {
                    Some((accent, under)) => {
                        chain += 1;
                        if chain > MAX_CHAIN {
                            break Err(MtefError::Nesting);
                        }
                        dressed = Node::Accent(Box::new(Accent { kind: accent, base: dressed, under }));
                    }
                    None => self.report.skipped += 1,
                }
                continue;
            }
            self.item(false)?;
            self.report.skipped += 1;
        };
        self.leave();
        result
    }

    fn embellishment_type(&mut self) -> Result<u8, MtefError> {
        let options = self.reader.u8()?;
        if options & OPT_NUDGE != 0 {
            self.reader.nudge()?;
        }
        self.reader.u8()
    }

    fn ruler(&mut self) -> Result<(), MtefError> {
        let stops = self.reader.u8()?;
        for _ in 0..stops {
            self.reader.u8()?;
            self.reader.u16()?;
        }
        Ok(())
    }

    fn size(&mut self, tag: u8) -> Result<MathStyle, MtefError> {
        let first = if tag == TAG_SIZE { self.reader.u8()? } else { tag - TAG_TYPESIZE_FIRST };
        let simple = match first {
            101 => {
                // An explicit point size, which the equation tree does not carry.
                self.reader.u16()?;
                return Ok(MathStyle::Full);
            }
            100 => {
                let size = self.reader.u8()?;
                self.reader.u16()?;
                size
            }
            size => size,
        };
        Ok(match simple {
            1 | 4 => MathStyle::Script,
            2 => MathStyle::ScriptScript,
            _ => MathStyle::Full,
        })
    }

    fn color_definition(&mut self) -> Result<(), MtefError> {
        let options = self.reader.u8()?;
        let values = if options & OPT_COLOR_CMYK != 0 { 4 } else { 3 };
        for _ in 0..values {
            self.reader.u16()?;
        }
        if options & OPT_COLOR_NAME != 0 {
            self.reader.cstring()?;
        }
        Ok(())
    }

    /// Read the equation preferences: the typesizes, spacing and styles of the stream.
    fn equation_preferences(&mut self) -> Result<(), MtefError> {
        let _options = self.reader.u8()?;
        let sizes = self.dimension_array()?;
        let spaces = self.dimension_array()?;
        let mut styles = Vec::new();
        let count = self.reader.u8()?;
        for _ in 0..count {
            let font = self.reader.unsigned()?;
            let style = if font == 0 { 0 } else { self.reader.u8()? };
            styles.push(StyleDef { font: (font != 0).then_some(font), style });
        }
        self.header.preferences = Preferences { sizes, spaces, styles };
        Ok(())
    }

    /// Read a dimension array: a byte count, then the values as nibbles.
    fn dimension_array(&mut self) -> Result<Vec<Dimension>, MtefError> {
        let count = self.reader.u8()?;
        let mut nibbles = Nibbles { data: self.reader.data, pos: self.reader.pos, high: false };
        let mut dimensions = Vec::new();
        for _ in 0..count {
            let unit = nibbles.next()?;
            dimensions.push(nibbles.dimension(unit)?);
        }
        // The nibble stream is padded out to a whole byte before the next field.
        self.reader.pos = nibbles.pos + usize::from(nibbles.high);
        Ok(dimensions)
    }

    // ------------------------------------------------------------- templates

    fn template(&mut self) -> Result<Item, MtefError> {
        let options = self.reader.u8()?;
        if options & OPT_NUDGE != 0 {
            self.reader.nudge()?;
        }
        let selector = self.reader.u8()?;
        let first = self.reader.u8()?;
        let variation = if first & 0x80 != 0 {
            u32::from(first & 0x7F) | (u32::from(self.reader.u8()?) << 8)
        } else {
            u32::from(first)
        };
        let _template_options = self.reader.u8()?;
        let items = self.items()?;
        let mut lines: Vec<(Row, bool)> = Vec::new();
        let mut glyphs: Vec<Node> = Vec::new();
        for item in items {
            match item {
                Item::Line { row, null } => lines.push((row, null)),
                Item::Nodes(nodes) => {
                    for node in nodes {
                        match node {
                            Node::Char(_) => glyphs.push(node),
                            Node::Row(row) => lines.push((row, false)),
                            other => lines.push((vec![other], false)),
                        }
                    }
                }
                Item::Script { sub, sup, precedes: false } => {
                    let base = lines.pop().map(|(row, _)| slot(row)).unwrap_or_else(Node::empty);
                    lines.push((vec![attach(base, sub, sup)], false));
                }
                Item::Script { sub, sup, precedes: true } => match lines.first_mut() {
                    Some((row, _)) => {
                        let base = slot(std::mem::take(row));
                        *row = vec![attach(base, sub, sup)];
                    }
                    None => lines.push((vec![attach(Node::empty(), sub, sup)], false)),
                },
            }
        }
        Ok(self.build_template(selector, variation, lines, glyphs))
    }

    fn build_template(&mut self, selector: u8, variation: u32, lines: Vec<(Row, bool)>, glyphs: Vec<Node>) -> Item {
        let glyph = |position: usize| {
            glyphs.get(position).and_then(|node| match node {
                Node::Char(ch) => Some(ch.ch),
                _ => None,
            })
        };
        // An optional slot is absent when its line is not drawn; a line that is drawn but empty
        // is a slot that is present and empty.
        let optional_line = |position: usize| lines.get(position).and_then(|(row, null)| {
            if *null {
                None
            } else {
                Some(slot(row.clone()))
            }
        });
        let main = lines.first().cloned().map(|(row, _)| row).unwrap_or_default();
        let second = lines.get(1).cloned().map(|(row, _)| row).unwrap_or_default();
        let third = lines.get(2).cloned().map(|(row, _)| row).unwrap_or_default();
        match selector {
            // Fences and intervals.
            0..=8 => {
                let left = variation & 0x0001 != 0;
                let right = variation & 0x0002 != 0;
                let (mut open, mut close) = delimiter_of(selector, left, right);
                // MathType writes the fence characters themselves; they refine the delimiter
                // only when they are not the ones the selector draws.
                match (glyph(0), left, right) {
                    (Some(ch), true, _) => open = refine_delim(open, ch, true),
                    (Some(ch), false, true) => close = refine_delim(close, ch, false),
                    _ => {}
                }
                if left && right {
                    if let Some(ch) = glyph(1) {
                        close = refine_delim(close, ch, false);
                    }
                }
                Item::Nodes(vec![fence(open, close, slot(main))])
            }
            9 => {
                let open = glyph(0).map_or_else(|| interval_delim(variation, true), |ch| refine_delim(interval_delim(variation, true), ch, true));
                let close = glyph(1).map_or_else(|| interval_delim(variation, false), |ch| refine_delim(interval_delim(variation, false), ch, false));
                Item::Nodes(vec![fence(open, close, slot(main))])
            }
            // Radical.
            10 => {
                let radicand = slot(main);
                let index = (variation & 0x0001 != 0).then(|| slot(second));
                Item::Nodes(vec![Node::Sqrt(Box::new(Sqrt { index, radicand }))])
            }
            // Fractions.
            11 => {
                let kind = match variation & 0x0006 {
                    0x0002 => FracKind::Slash,
                    0x0004 => FracKind::Linear,
                    0x0006 => FracKind::NoBar,
                    _ => FracKind::Bar,
                };
                Item::Nodes(vec![Node::Frac(Box::new(Frac { num: slot(main), den: slot(second), kind }))])
            }
            // Accents, horizontal braces and brackets.
            12 | 13 | 14 | 24 | 25 | 31 | 32 | 33 | 34 | 36 => {
                let (mut kind, mut under) = accent_of_template(selector, variation);
                if let Some((accent, accent_under)) = glyph(0).and_then(accent_of_glyph) {
                    kind = accent;
                    // A template that draws on a side of its own keeps it; the rest of them
                    // take the side from the glyph they carry.
                    if !template_has_side(selector) {
                        under = accent_under;
                    }
                }
                if selector == 14 && lines.len() >= 2 {
                    // An arrow with material on both sides of it is over/under material.
                    return Item::Nodes(vec![Node::Limits(Box::new(Limits {
                        base: slot(main),
                        over: optional(second),
                        under: optional(third),
                        align: Side::Center,
                    }))]);
                }
                if selector == 24 || selector == 25 {
                    // A labelled brace keeps its label as over/under material.
                    if let Some(label) = optional(second) {
                        let base = Node::Accent(Box::new(Accent { kind, base: slot(main), under }));
                        let (over, under) = if under { (None, Some(label)) } else { (Some(label), None) };
                        return Item::Nodes(vec![Node::Limits(Box::new(Limits {
                            base,
                            over,
                            under,
                            align: Side::Center,
                        }))]);
                    }
                }
                Item::Nodes(vec![Node::Accent(Box::new(Accent { kind, base: slot(main), under }))])
            }
            // Large operators: the operand follows the operator in the tree, while the limits
            // and the operator glyph live in the template.
            15..=22 => {
                let base = glyphs.last().cloned().unwrap_or_else(|| {
                    let ch = default_operator(selector);
                    Node::Char(mt_core::Char { ch, class: Class::Op, font: Font::Upright })
                });
                let big = BigOp {
                    base,
                    sub: optional_line(1),
                    sup: optional_line(2),
                    limits: variation & 0x0040 != 0,
                };
                let mut nodes = vec![Node::Big(Box::new(big))];
                nodes.extend(main);
                Item::Nodes(nodes)
            }
            // Limits.
            23 => Item::Nodes(vec![Node::Limits(Box::new(Limits {
                base: slot(main),
                under: optional_line(1),
                over: optional_line(2),
                align: Side::Center,
            }))]),
            // Long division.
            26 => Item::Nodes(vec![Node::Matrix(Box::new(Matrix {
                rows: vec![vec![slot(main)], vec![slot(second)]],
                delims: None,
                kind: MatrixKind::LongDivision,
                align: Side::Center,
                columns: Vec::new(),
                rules: false,
            }))]),
            // Scripts: the base is the neighbouring record, the slots are sub then superscript.
            27..=29 => Item::Script {
                sub: optional_line(0),
                sup: optional_line(1),
                precedes: variation & 0x0001 != 0,
            },
            // Bra-ket notation.
            30 => {
                let mut body = vec![slot(main), Node::symbol('|', Class::Ord)];
                if let Some(right) = optional(second) {
                    body.push(right);
                }
                let open = if variation & 0x0001 != 0 { Delim::Angle } else { Delim::None };
                let close = if variation & 0x0002 != 0 { Delim::Angle } else { Delim::None };
                Item::Nodes(vec![Node::Fence(Box::new(Fence { open, close, body: Node::Row(body) }))])
            }
            // Boxes.
            37 => Item::Nodes(vec![fence(Delim::None, Delim::None, slot(main))]),
            // A template this crate does not know: keep its contents.
            _ => {
                self.report.skipped += 1;
                Item::Nodes(lines.into_iter().flat_map(|(row, _)| row).collect())
            }
        }
    }

    // ---------------------------------------------------------------- piles

    fn pile(&mut self) -> Result<Item, MtefError> {
        let options = self.reader.u8()?;
        if options & OPT_NUDGE != 0 {
            self.reader.nudge()?;
        }
        let horizontal = self.reader.u8()?;
        let _vertical = self.reader.u8()?;
        if options & OPT_LP_RULER != 0 {
            self.enter()?;
            let result = self.item(true);
            self.leave();
            result?;
        }
        let rows = self.rows()?;
        Ok(Item::Nodes(vec![Node::Matrix(Box::new(Matrix {
            rows,
            delims: None,
            kind: MatrixKind::Pile,
            align: side_of(horizontal),
            columns: Vec::new(),
            rules: false,
        }))]))
    }

    /// Read one row per line until the next `END`, skipping records between the lines.
    fn rows(&mut self) -> Result<Vec<Row>, MtefError> {
        self.enter()?;
        let result = (|| {
            let mut rows = Vec::new();
            loop {
                if self.reader.peek()? == TAG_END {
                    self.reader.u8()?;
                    self.report.records += 1;
                    return Ok(rows);
                }
                match self.item(true)? {
                    Item::Line { row, .. } => rows.push(row),
                    Item::Nodes(nodes) if !nodes.is_empty() => rows.push(nodes),
                    Item::Nodes(_) => self.report.skipped += 1,
                    Item::Script { sub, sup, .. } => {
                        rows.push(vec![attach(Node::empty(), sub, sup)]);
                    }
                }
            }
        })();
        self.leave();
        result
    }

    // --------------------------------------------------------------- tables

    fn matrix(&mut self) -> Result<Item, MtefError> {
        let options = self.reader.u8()?;
        if options & OPT_NUDGE != 0 {
            self.reader.nudge()?;
        }
        let _vertical = self.reader.u8()?;
        let horizontal = self.reader.u8()?;
        let _column_vertical = self.reader.u8()?;
        let row_count = usize::from(self.reader.u8()?);
        let column_count = usize::from(self.reader.u8()?);
        let row_parts = self.reader.bytes((row_count + 1).div_ceil(4))?.to_vec();
        let column_parts = self.reader.bytes((column_count + 1).div_ceil(4))?.to_vec();
        let rules = row_parts.iter().any(|&byte| byte != 0)
            || column_parts.iter().any(|&byte| byte != 0);
        let rows = self.cells(row_count, column_count)?;
        let kind = if rules { MatrixKind::Array } else { MatrixKind::Matrix };
        Ok(Item::Nodes(vec![Node::Matrix(Box::new(Matrix {
            rows,
            delims: None,
            kind,
            align: side_of(horizontal),
            columns: Vec::new(),
            rules,
        }))]))
    }

    /// Read a table's cells: the object list holds one line per cell, in row-major order.
    fn cells(&mut self, row_count: usize, column_count: usize) -> Result<Vec<Row>, MtefError> {
        self.enter()?;
        let result = (|| {
            let mut rows = Vec::with_capacity(row_count.min(64));
            for _ in 0..row_count {
                let mut row = Vec::with_capacity(column_count.min(64));
                for _ in 0..column_count {
                    row.push(slot(self.cell()?));
                }
                rows.push(row);
            }
            // Anything else in the list is a record the table does not use.
            loop {
                if self.reader.peek()? == TAG_END {
                    self.reader.u8()?;
                    self.report.records += 1;
                    return Ok(rows);
                }
                self.item(true)?;
                self.report.skipped += 1;
            }
        })();
        self.leave();
        result
    }

    /// Read one cell of a table, skipping the records that are not lines.
    fn cell(&mut self) -> Result<Row, MtefError> {
        loop {
            match self.item(true)? {
                Item::Line { row, .. } => return Ok(row),
                Item::Nodes(nodes) if nodes.is_empty() => self.report.skipped += 1,
                Item::Nodes(nodes) => return Ok(nodes),
                Item::Script { sub, sup, .. } => {
                    return Ok(vec![attach(Node::empty(), sub, sup)]);
                }
            }
        }
    }

    // ----------------------------------------------------------------- fonts

    /// The face a typeface names, resolved through the font definitions of the stream.
    fn font_of(&self, typeface: i32) -> Font {
        if typeface >= 0 {
            let index = usize::try_from(typeface).unwrap_or(0);
            let style = index
                .checked_sub(1)
                .and_then(|index| self.header.preferences.styles.get(index));
            if let Some(style) = style {
                if let Some(definition) = style.font.and_then(|font| self.header.fonts.get(font - 1)) {
                    return font_from(&definition.name, style.style);
                }
            }
            style_font(typeface)
        } else {
            let index = usize::try_from(-typeface).unwrap_or(0);
            let definition = index
                .checked_sub(1)
                .and_then(|index| self.font_styles.get(index))
                .and_then(|&(font, style)| self.header.fonts.get(font.saturating_sub(1)).map(|def| (def, style)));
            match definition {
                Some((definition, style)) => font_from(&definition.name, style),
                None => Font::Upright,
            }
        }
    }
}

// ------------------------------------------------------------------------------ slots

/// The value of a slot: an empty row, the single node, or a row of several nodes.
fn slot(row: Row) -> Node {
    match row.len() {
        0 => Node::empty(),
        1 => row.into_iter().next().unwrap(),
        _ => Node::Row(row),
    }
}

/// The value of an optional slot: absent when the line is empty.
fn optional(row: Row) -> Option<Node> {
    if row.is_empty() {
        None
    } else {
        Some(slot(row))
    }
}

/// A node with scripts attached.
fn attach(base: Node, sub: Option<Node>, sup: Option<Node>) -> Node {
    Node::Scripts(Box::new(Scripts { base, sub, sup }))
}

/// A fence around `body`, folded into the table when the body is exactly one.
fn fence(open: Delim, close: Delim, body: Node) -> Node {
    match body {
        Node::Matrix(mut matrix) if matrix.delims.is_none() => {
            matrix.delims = Some((open, close));
            if open == Delim::Brace && close == Delim::None && matrix.align == Side::Left {
                matrix.kind = MatrixKind::Cases;
            }
            Node::Matrix(matrix)
        }
        body => Node::Fence(Box::new(Fence { open, close, body })),
    }
}

/// The delimiter a fence character stands for inside a fence of `delimiter`'s family.
fn refine_delim(delimiter: Delim, glyph: char, opening: bool) -> Delim {
    match family_of(delimiter) {
        Some((_, natural_open, natural_close)) => {
            let natural = if opening { natural_open } else { natural_close };
            if glyph == natural { delimiter } else { Delim::Custom(glyph) }
        }
        None => Delim::Custom(glyph),
    }
}
