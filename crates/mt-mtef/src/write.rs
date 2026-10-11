//! Writing the equation tree as an equation byte stream.
//!
//! The writer emits a complete definition section — one encoding definition, the font
//! definitions the named styles refer to, the equation preferences and one font style
//! definition per extra face the tree uses — followed by the equation itself. Only fields whose
//! meaning is established are written, and the reader undoes exactly this encoding.

use mt_core::{Class, Delim, Font, FracKind, Math, MathStyle, Matrix, MatrixKind, Node, Row};

use crate::tables::*;
use crate::Unit;

/// The twenty-four bytes an equation object carries in front of the equation data.
///
/// They describe the object's frame rather than the equation, and are written unchanged so that
/// a stream this crate produces can be embedded wherever an object written by MathType can.
const HOST_HEADER: [u8; 24] = [
    0x02, 0x00, 0x22, 0xC3, 0xC0, 0x00, 0x00, 0x00, 0x54, 0x00, 0xE3, 0x00, 0x50, 0xF1, 0x55,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x00, 0xE3, 0x00,
];

/// Length of the preamble a written stream opens with: the length field and the host header.
pub(crate) const STREAM_HEADER_LENGTH: u32 = 28;

/// A complete stream: the preamble followed by the equation data.
pub(crate) fn stream(math: &Math) -> Vec<u8> {
    let body = document(math);
    let mut out = Vec::with_capacity(body.len() + STREAM_HEADER_LENGTH as usize);
    out.extend_from_slice(&STREAM_HEADER_LENGTH.to_le_bytes());
    out.extend_from_slice(&HOST_HEADER);
    out.extend_from_slice(&body);
    out
}

/// A bare equation byte stream, without the preamble.
pub(crate) fn document(math: &Math) -> Vec<u8> {
    let mut fonts = Vec::new();
    collect_fonts(&math.body, &mut fonts);
    let mut writer = Writer { out: Vec::new(), fonts };
    writer.header();
    writer.definitions();
    writer.size(math.style);
    writer.write_line(&Slot::Node(&math.body));
    writer.out.push(TAG_END);
    writer.out
}

/// One slot of a template: a line, or a line that is not drawn.
enum Slot<'a> {
    /// A line with no object list, i.e. a slot that is not drawn.
    Absent,
    /// A line holding one node.
    Node(&'a Node),
    /// A line holding a row's items.
    Row(&'a Row),
    /// A line holding already encoded records, used for a table written inside delimiters.
    Ordered(&'a [u8]),
}

struct Writer {
    out: Vec<u8>,
    /// Extra faces the tree uses, in first-use order. Index `i` is font definition `5 + i` and
    /// font style `i + 1`.
    fonts: Vec<(String, u8)>,
}

impl Writer {
    // ------------------------------------------------------------- sections

    fn header(&mut self) {
        // Version, platform (Windows), product (MathType), product version and subversion,
        // application key, equation options.
        self.out.extend_from_slice(&[5, 1, 0, 6, 5]);
        self.out.extend_from_slice(b"DSMT6\0");
        self.out.push(0);
    }

    fn definitions(&mut self) {
        self.out.push(TAG_ENCODING_DEF);
        self.string(DEFAULT_ENCODING);
        for (name, encoding) in STANDARD_FONTS {
            self.out.push(TAG_FONT_DEF);
            self.unsigned(encoding);
            self.string(name);
        }
        let extras = self.fonts.clone();
        for (name, _) in &extras {
            self.out.push(TAG_FONT_DEF);
            self.unsigned(ENC_FIRST_DEFINED);
            self.string(name);
        }
        for (position, (_, style)) in extras.iter().enumerate() {
            self.out.push(TAG_FONT_STYLE_DEF);
            self.unsigned(STANDARD_FONTS.len() + 1 + position);
            self.out.push(*style);
        }
        self.preferences();
    }

    fn preferences(&mut self) {
        self.out.push(TAG_EQN_PREFS);
        self.out.push(0);
        self.dimensions(&DEFAULT_SIZES);
        self.dimensions(&DEFAULT_SPACES);
        self.out.push(DEFAULT_STYLES.len() as u8);
        for (font, style) in DEFAULT_STYLES {
            match font {
                Some(index) => {
                    self.unsigned(index);
                    self.out.push(style);
                }
                None => self.unsigned(0),
            }
        }
    }

    fn size(&mut self, style: MathStyle) {
        self.out.push(match style {
            MathStyle::Script => TAG_TYPESIZE_FIRST + 1,
            MathStyle::ScriptScript => TAG_TYPESIZE_FIRST + 2,
            // Text size has no record of its own; the equation is written at full size.
            MathStyle::Text | MathStyle::Full => TAG_TYPESIZE_FIRST,
        });
    }

    // --------------------------------------------------------------- nodes

    fn node(&mut self, node: &Node) {
        match node {
            Node::Row(row) => {
                for item in row {
                    self.node(item);
                }
            }
            Node::Char(ch) => self.character(ch.ch, ch.class, ch.font, None),
            Node::Text(text) => {
                for ch in text.text.chars() {
                    self.character(ch, class_of_char(ch), text.font, None);
                }
            }
            // Explicit spacing has no record of its own; it is written as a space in the
            // spacing style.
            Node::Space(_) => self.character(' ', Class::Ord, Font::Upright, Some(TF_SPACE)),
            // An alignment point has no spelling in this format.
            Node::Align => {}
            Node::Frac(frac) => {
                let variation = match frac.kind {
                    FracKind::Bar => 0,
                    FracKind::Slash => 0x0002,
                    FracKind::Linear => 0x0004,
                    FracKind::NoBar => 0x0006,
                };
                self.template(11, variation, &[Slot::Node(&frac.num), Slot::Node(&frac.den)], &[]);
            }
            Node::Sqrt(sqrt) => {
                let index = match &sqrt.index {
                    Some(index) => Slot::Node(index),
                    None => Slot::Absent,
                };
                self.template(10, u32::from(sqrt.index.is_some()), &[Slot::Node(&sqrt.radicand), index], &[]);
            }
            Node::Scripts(scripts) => {
                self.node(&scripts.base);
                match (&scripts.sub, &scripts.sup) {
                    (None, None) => {}
                    (Some(sub), None) => self.template(27, 0, &[Slot::Node(sub)], &[]),
                    (None, Some(sup)) => self.template(28, 0, &[Slot::Absent, Slot::Node(sup)], &[]),
                    (Some(sub), Some(sup)) => {
                        self.template(29, 0, &[Slot::Node(sub), Slot::Node(sup)], &[]);
                    }
                }
            }
            Node::Limits(limits) => {
                let under = optional_slot(limits.under.as_ref());
                let over = optional_slot(limits.over.as_ref());
                self.template(23, 0, &[Slot::Node(&limits.base), under, over], &[]);
            }
            Node::Fence(fence) => self.fence(fence.open, fence.close, Slot::Node(&fence.body)),
            Node::Matrix(matrix) => self.matrix(matrix),
            Node::Accent(_) => {
                // An accent on a single character is written the way MathType writes it, as an
                // embellishment of that character.
                if let Some((ch, embellishments)) = embellishment_chain(node) {
                    self.embellished_character(ch.ch, ch.class, ch.font, &embellishments);
                    return;
                }
                if let Node::Accent(accent) = node {
                    let (selector, variation, glyph) = template_of_accent(accent.kind, accent.under);
                    self.template(selector, variation, &[Slot::Node(&accent.base)], &[glyph]);
                }
            }
            Node::Big(big) => self.big_op(big),
            // A style override has no record of its own; the body is written as it stands.
            Node::Styled(styled) => self.node(&styled.body),
            // A construct this crate could not represent is written as its text.
            Node::Error(text) => {
                for ch in text.chars() {
                    self.character(ch, class_of_char(ch), Font::Upright, None);
                }
            }
        }
    }

    /// Write a fence around a slot. A fence that draws nothing is written as a box.
    fn fence(&mut self, open: Delim, close: Delim, body: Slot<'_>) {
        if open == Delim::None && close == Delim::None {
            self.template(37, 0, &[body], &[]);
            return;
        }
        let left = open != Delim::None;
        let right = close != Delim::None;
        let (selector, variation, glyphs) = fence_template(open, close, left, right);
        self.template(selector, variation, &[body], &glyphs);
    }

    fn matrix(&mut self, matrix: &Matrix) {
        let records = self.matrix_records(matrix);
        match matrix.delims {
            // The table is written inside its delimiters; the reader folds the pair back into
            // the table.
            Some((open, close)) => self.fence(open, close, Slot::Ordered(&records)),
            None => self.out.extend_from_slice(&records),
        }
    }

    /// The records of a table, whatever its kind: a table, a stack or a long division.
    fn matrix_records(&self, matrix: &Matrix) -> Vec<u8> {
        if matrix.kind == MatrixKind::LongDivision {
            let dividend = matrix.rows.first().map_or(Slot::Absent, Slot::Row);
            let quotient = matrix.rows.get(1).map_or(Slot::Absent, Slot::Row);
            let variation = u32::from(!matches!(quotient, Slot::Absent));
            let mut writer = self.blank();
            writer.template(26, variation, &[dividend, quotient], &[]);
            return writer.out;
        }
        if matrix.kind == MatrixKind::Pile {
            let mut writer = self.blank();
            writer.out.push(TAG_PILE);
            writer.out.push(0);
            writer.out.push(side_value(matrix.align));
            writer.out.push(2);
            for row in &matrix.rows {
                writer.write_line(&Slot::Row(row));
            }
            writer.out.push(TAG_END);
            return writer.out;
        }
        self.matrix_record(matrix)
    }

    /// A writer that shares this one's font definitions and collects records of its own.
    fn blank(&self) -> Writer {
        Writer { out: Vec::new(), fonts: self.fonts.clone() }
    }

    /// A table record, whose cell lines carry the writer's font definitions.
    fn matrix_record(&self, matrix: &Matrix) -> Vec<u8> {
        let columns = matrix.col_count();
        let mut out = vec![
            TAG_MATRIX,
            0,
            // The vertical alignment within the container and the horizontal alignment of the
            // columns; the tree carries only the horizontal one.
            4,
            side_value(matrix.align),
            2,
            matrix.row_count().min(255) as u8,
            columns.min(255) as u8,
        ];
        let partition = if matrix.rules { 0xFF } else { 0x00 };
        out.extend(std::iter::repeat_n(partition, (matrix.row_count() + 1).div_ceil(4)));
        out.extend(std::iter::repeat_n(partition, (columns + 1).div_ceil(4)));
        let mut cells = self.blank();
        for row in &matrix.rows {
            for cell in row {
                cells.write_line(&Slot::Node(cell));
            }
            for _ in row.len()..columns {
                cells.write_line(&Slot::Absent);
            }
        }
        out.extend_from_slice(&cells.out);
        out.push(TAG_END);
        out
    }

    fn big_op(&mut self, big: &mt_core::BigOp) {
        let operator = match &big.base {
            Node::Char(ch) => Some(ch.ch),
            _ => None,
        };
        let selector = operator.map_or(22, big_selector);
        let mut variation = 0;
        if big.limits {
            variation |= 0x0040;
        }
        if big.sub.is_some() {
            variation |= 0x0010;
        }
        if big.sup.is_some() {
            variation |= 0x0020;
        }
        if selector == 15 {
            variation |= match operator {
                Some('\u{222C}') => 0x0002,
                Some('\u{222D}') => 0x0003,
                Some('\u{222E}') | Some('\u{222F}') | Some('\u{2230}') => 0x0004,
                _ => 0x0001,
            };
        }
        let under = optional_slot(big.sub.as_ref());
        let over = optional_slot(big.sup.as_ref());
        // The operand of an operator with limits is a sibling in the equation tree, so the main
        // slot is left out; an operator that is not a character keeps its contents there.
        let main = match operator {
            Some(_) => Slot::Absent,
            None => Slot::Node(&big.base),
        };
        self.out.push(TAG_TMPL);
        self.out.push(0);
        self.out.push(selector);
        self.push_variation(variation);
        self.out.push(0);
        self.write_line(&main);
        self.write_line(&under);
        self.write_line(&over);
        if let Some(ch) = operator {
            let (class, font) = match &big.base {
                Node::Char(ch) => (ch.class, ch.font),
                _ => (Class::Op, Font::Upright),
            };
            self.character(ch, class, font, None);
        }
        self.out.push(TAG_END);
    }

    // -------------------------------------------------------------- records

    /// Write a template record: its subobject lines and its decorative characters.
    fn template(&mut self, selector: u8, variation: u32, lines: &[Slot<'_>], glyphs: &[Option<char>]) {
        self.out.push(TAG_TMPL);
        self.out.push(0);
        self.out.push(selector);
        self.push_variation(variation);
        self.out.push(0);
        for slot in lines {
            self.write_line(slot);
        }
        for ch in glyphs.iter().flatten() {
            self.character(*ch, class_of_char(*ch), Font::Upright, Some(TF_EXPAND));
        }
        self.out.push(TAG_END);
    }

    /// Write a line record. [`Slot::Absent`] writes a line that is not drawn.
    fn write_line(&mut self, contents: &Slot<'_>) {
        self.out.push(TAG_LINE);
        match contents {
            Slot::Absent => {
                self.out.push(OPT_LINE_NULL);
                return;
            }
            Slot::Node(node) => {
                self.out.push(0);
                self.node(node);
            }
            Slot::Row(row) => {
                self.out.push(0);
                for item in *row {
                    self.node(item);
                }
            }
            Slot::Ordered(records) => {
                self.out.push(0);
                self.out.extend_from_slice(records);
            }
        }
        self.out.push(TAG_END);
    }

    fn character(&mut self, ch: char, class: Class, font: Font, typeface: Option<i32>) {
        self.out.push(TAG_CHAR);
        self.out.push(0);
        let typeface = typeface.unwrap_or_else(|| self.typeface(font, class));
        self.signed(typeface);
        self.u16(u16::try_from(ch as u32).unwrap_or(0xFFFD));
    }

    /// Write a character followed by its embellishment list, which decorates it.
    fn embellished_character(&mut self, ch: char, class: Class, font: Font, embellishments: &[u8]) {
        self.out.push(TAG_CHAR);
        self.out.push(OPT_CHAR_EMBELL);
        let typeface = self.typeface(font, class);
        self.signed(typeface);
        self.u16(u16::try_from(ch as u32).unwrap_or(0xFFFD));
        for embellishment in embellishments {
            self.out.push(TAG_EMBELL);
            self.out.push(0);
            self.out.push(*embellishment);
        }
        self.out.push(TAG_END);
    }

    fn string(&mut self, text: &str) {
        for ch in text.chars() {
            self.out.push(u8::try_from(ch as u32).unwrap_or(b'?'));
        }
        self.out.push(0);
    }

    fn push_variation(&mut self, variation: u32) {
        if variation & 0x7F80 == 0 {
            self.out.push(variation as u8);
        } else {
            self.out.push((variation as u8 & 0x7F) | 0x80);
            self.out.push((variation >> 8) as u8);
        }
    }

    /// A dimension array: a count, then the units and digits of each dimension as nibbles.
    fn dimensions(&mut self, dimensions: &[(Unit, &str)]) {
        self.out.push(dimensions.len() as u8);
        let mut nibbles = Vec::new();
        for (unit, value) in dimensions {
            nibbles.push(unit.code());
            for ch in value.chars() {
                match ch {
                    '0'..='9' => nibbles.push(ch as u8 - b'0'),
                    '.' => nibbles.push(0x0A),
                    '-' => nibbles.push(0x0B),
                    _ => {}
                }
            }
            nibbles.push(0x0F);
        }
        if nibbles.len() % 2 != 0 {
            nibbles.push(0);
        }
        for pair in nibbles.chunks(2) {
            self.out.push((pair[0] << 4) | pair[1]);
        }
    }

    fn unsigned(&mut self, value: usize) {
        match u8::try_from(value) {
            Ok(byte) if byte < 0xFF => self.out.push(byte),
            _ => {
                self.out.push(0xFF);
                self.u16(u16::try_from(value).unwrap_or(0xFFFF));
            }
        }
    }

    fn signed(&mut self, value: i32) {
        if (-128..127).contains(&value) {
            self.out.push((value + 128) as u8);
        } else {
            self.out.push(0xFF);
            self.u16((value + 32768) as u16);
        }
    }

    fn u16(&mut self, value: u16) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    /// The typeface a character is written with.
    fn typeface(&self, font: Font, class: Class) -> i32 {
        standard_typeface(font, class).unwrap_or_else(|| {
            let (name, bits) = explicit_font(font);
            let index = self
                .fonts
                .iter()
                .position(|(seen, seen_bits)| *seen == name && *seen_bits == bits)
                .map_or(1, |position| position + 1);
            -(index as i32)
        })
    }
}

/// The character an accent chain decorates, with the embellishments that decorate it, innermost
/// first — the order the reader applies them in.
fn embellishment_chain(node: &Node) -> Option<(&mt_core::Char, Vec<u8>)> {
    let mut embellishments = Vec::new();
    let mut current = node;
    loop {
        match current {
            Node::Accent(accent) => {
                embellishments.push(embellishment_of_accent(accent.kind, accent.under)?);
                current = &accent.base;
            }
            Node::Char(ch) => {
                embellishments.reverse();
                return Some((ch, embellishments));
            }
            _ => return None,
        }
    }
}

/// The template selector, variation and decorative characters of a fence.
fn fence_template(open: Delim, close: Delim, left: bool, right: bool) -> (u8, u32, [Option<char>; 2]) {
    match (family_of(open), family_of(close)) {
        (Some(open_family), Some(close_family)) if left && right && open_family.0 == close_family.0 => {
            (open_family.0, 0x0003, [Some(open_family.1), Some(close_family.2)])
        }
        (Some(family), _) if !right => (family.0, 0x0001, [Some(family.1), None]),
        (_, Some(family)) if !left => (family.0, 0x0002, [Some(family.2), None]),
        _ => {
            let variation =
                interval_code(open, true).unwrap_or(0) | interval_code(close, false).unwrap_or(0);
            (9, variation, [delimiter_char(open, true), delimiter_char(close, false)])
        }
    }
}

/// The character a delimiter is drawn with on one side.
fn delimiter_char(delim: Delim, opening: bool) -> Option<char> {
    match delim {
        Delim::None => None,
        Delim::Custom(ch) => Some(ch),
        delim => {
            if opening {
                delim.open()
            } else {
                delim.close()
            }
        }
    }
}

/// A line that holds a node, or nothing.
fn optional_slot(node: Option<&Node>) -> Slot<'_> {
    match node {
        Some(node) => Slot::Node(node),
        None => Slot::Absent,
    }
}

/// The template selector of a large operator.
pub(crate) fn big_selector(ch: char) -> u8 {
    match ch {
        '\u{222B}' | '\u{222C}' | '\u{222D}' | '\u{222E}' | '\u{222F}' | '\u{2230}' => 15,
        '\u{2211}' => 16,
        '\u{220F}' => 17,
        '\u{2210}' => 18,
        '\u{22C3}' => 19,
        '\u{22C2}' => 20,
        _ => 22,
    }
}

/// Collect the faces a tree needs font definitions for, in first-use order.
fn collect_fonts(node: &Node, fonts: &mut Vec<(String, u8)>) {
    node.walk(&mut |node| {
        let font = match node {
            Node::Char(ch) => Some(ch.font),
            Node::Text(text) => Some(text.font),
            Node::Space(_) => Some(Font::Upright),
            _ => None,
        };
        if let Some(font) = font {
            if standard_typeface(font, Class::Ord).is_none() {
                let (name, bits) = explicit_font(font);
                if !fonts.iter().any(|(seen, seen_bits)| seen == name && *seen_bits == bits) {
                    fonts.push((name.to_owned(), bits));
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writer() -> Writer {
        Writer { out: Vec::new(), fonts: Vec::new() }
    }

    #[test]
    fn values_are_written_in_their_short_or_long_form() {
        let mut writer = writer();
        writer.signed(3);
        writer.signed(-3);
        writer.signed(200);
        writer.signed(-400);
        writer.unsigned(4);
        writer.unsigned(300);
        // Biased by 128 in one byte; `0xFF`, a low byte and a high byte when it does not fit.
        assert_eq!(
            writer.out,
            vec![131, 125, 0xFF, 0xC8, 0x80, 0xFF, 0x70, 0x7E, 4, 0xFF, 0x2C, 0x01]
        );
    }

    #[test]
    fn a_wide_variation_takes_two_bytes() {
        let mut writer = writer();
        writer.push_variation(0x70);
        assert_eq!(writer.out, vec![0x70], "the variations in use fit in one byte");

        writer.out.clear();
        writer.push_variation(0x0123);
        assert_eq!(writer.out, vec![0xA3, 0x01]);
    }

    #[test]
    fn dimensions_are_packed_two_to_a_byte() {
        let mut writer = writer();
        writer.dimensions(&[(Unit::Points, "12"), (Unit::Percent, "0.5")]);
        // A unit nibble, the digits of the value, a terminator nibble, and a padding nibble to
        // fill the last byte out.
        assert_eq!(writer.out, vec![2, 0x21, 0x2F, 0x40, 0xA5, 0xF0]);
    }
}
