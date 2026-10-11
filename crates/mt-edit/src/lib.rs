//! Editing model: where the caret sits in the equation tree, how typing and palette commands
//! mutate it, and how those mutations are undone.
//!
//! The model is deliberately independent of any windowing toolkit: it owns an [`mt_core::Math`]
//! plus a caret, and every command returns nothing but leaves the state consistent.
//!
//! ```
//! use mt_edit::{Editor, Template};
//!
//! let mut editor = Editor::new();
//! editor.insert_char('a');
//! editor.insert_template(Template::Fraction);
//! editor.insert_char('1');
//! assert_eq!(editor.math().body.slot_count(), 2);
//! ```

pub mod palette;

use mt_core::{
    Accent, AccentKind, BigOp, Char, Class, Delim, Font, Frac, FracKind, Fence, Math, MathStyle,
    Matrix, MatrixKind, Node, Path, Row, Scripts, Side, Space, Sqrt, Styled, TextRun,
};

pub use palette::{Palette, PaletteItem, palettes};

/// Number of undo steps kept.
const HISTORY_LIMIT: usize = 256;

/// Caret position: a row, plus the insertion index inside that row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor {
    /// Path of the row the caret sits in.
    pub path: Path,
    /// Insertion index inside that row, `0..=row.len()`.
    pub index: usize,
}

impl Cursor {
    /// Caret at the start of the equation body.
    pub fn start() -> Self {
        Self { path: Path::root(), index: 0 }
    }

    /// Caret inside the slot `slot` of the node at `path`, at the start of that slot's row.
    pub fn in_slot(path: &Path, slot: usize) -> Self {
        Self { path: path.child(slot), index: 0 }
    }

    /// Caret in the current row, moved to `index`.
    pub fn at(&self, index: usize) -> Self {
        Self { path: self.path.clone(), index }
    }
}

/// A template the palette can insert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template {
    /// Stacked fraction with a rule.
    Fraction,
    /// Stacked fraction with a slash.
    SlashFraction,
    /// Binomial coefficient, stacked without a rule.
    Binomial,
    /// Square root.
    SquareRoot,
    /// Root with an index slot.
    NthRoot,
    Superscript,
    Subscript,
    SubSuperscript,
    /// Large operator taking limits above and below.
    Sum,
    Product,
    Coproduct,
    Integral,
    DoubleIntegral,
    TripleIntegral,
    ContourIntegral,
    /// Limit stacked under a base that has no operator of its own.
    LimitBelow,
    /// Material stacked above a base.
    LimitAbove,
    OverBar,
    UnderBar,
    OverBrace,
    UnderBrace,
    OverArrow,
    UnderArrow,
    /// Delimiters around a body.
    Fence(Delim, Delim),
    /// Table with the given shape.
    Matrix {
        rows: usize,
        cols: usize,
        kind: MatrixKind,
        delims: Option<(Delim, Delim)>,
    },
    /// Accent over a base.
    Accent(AccentKind),
    /// Upright function name followed by an empty argument.
    Function(&'static str),
    /// Vertical stack of rows, centred.
    Pile,
}

impl Template {
    /// Build the node and report the slot path the caret should move to, relative to the node.
    pub fn build(self) -> (Node, Vec<usize>) {
        match self {
            Template::Fraction => (
                Node::Frac(Box::new(Frac { num: Node::empty(), den: Node::empty(), kind: FracKind::Bar })),
                vec![0],
            ),
            Template::SlashFraction => (
                Node::Frac(Box::new(Frac { num: Node::empty(), den: Node::empty(), kind: FracKind::Slash })),
                vec![0],
            ),
            Template::Binomial => (
                Node::Frac(Box::new(Frac { num: Node::empty(), den: Node::empty(), kind: FracKind::NoBar })),
                vec![0],
            ),
            Template::SquareRoot => (Node::Sqrt(Box::new(Sqrt::square(Node::empty()))), vec![0]),
            Template::NthRoot => (
                Node::Sqrt(Box::new(Sqrt::nth(Node::empty(), Node::empty()))),
                vec![1],
            ),
            Template::Superscript => (
                Node::Scripts(Box::new(Scripts { base: Node::empty(), sub: None, sup: Some(Node::empty()) })),
                vec![2],
            ),
            Template::Subscript => (
                Node::Scripts(Box::new(Scripts { base: Node::empty(), sub: Some(Node::empty()), sup: None })),
                vec![1],
            ),
            Template::SubSuperscript => (
                Node::Scripts(Box::new(Scripts {
                    base: Node::empty(),
                    sub: Some(Node::empty()),
                    sup: Some(Node::empty()),
                })),
                vec![0],
            ),
            Template::Sum => (big_op('\u{2211}', true), vec![0]),
            Template::Product => (big_op('\u{220F}', true), vec![0]),
            Template::Coproduct => (big_op('\u{2210}', true), vec![0]),
            Template::Integral => (big_op('\u{222B}', false), vec![0]),
            Template::DoubleIntegral => (big_op('\u{222C}', false), vec![0]),
            Template::TripleIntegral => (big_op('\u{222D}', false), vec![0]),
            Template::ContourIntegral => (big_op('\u{222E}', false), vec![0]),
            Template::LimitBelow => (
                Node::Limits(Box::new(mt_core::Limits {
                    base: Node::empty(),
                    under: Some(Node::empty()),
                    over: None,
                    align: Side::Center,
                })),
                vec![0],
            ),
            Template::LimitAbove => (
                Node::Limits(Box::new(mt_core::Limits {
                    base: Node::empty(),
                    under: None,
                    over: Some(Node::empty()),
                    align: Side::Center,
                })),
                vec![0],
            ),
            Template::OverBar => (accent(AccentKind::Bar, false), vec![0]),
            Template::UnderBar => (accent(AccentKind::Bar, true), vec![0]),
            Template::OverBrace => (accent(AccentKind::OverBrace, false), vec![0]),
            Template::UnderBrace => (accent(AccentKind::UnderBrace, true), vec![0]),
            Template::OverArrow => (accent(AccentKind::ArrowRight, false), vec![0]),
            Template::UnderArrow => (accent(AccentKind::ArrowRight, true), vec![0]),
            Template::Fence(open, close) => (fence(open, close), vec![0]),
            Template::Matrix { rows, cols, kind, delims } => (
                Node::Matrix(Box::new(Matrix {
                    rows: vec![vec![Node::empty(); cols.max(1)]; rows.max(1)],
                    delims,
                    kind,
                    align: default_alignment(kind),
                    columns: Vec::new(),
                    rules: matches!(kind, MatrixKind::Array),
                })),
                vec![0],
            ),
            Template::Accent(kind) => (accent(kind, false), vec![0]),
            Template::Function(name) => (
                Node::row([
                    Node::Text(Box::new(TextRun { text: name.to_owned(), font: Font::Upright })),
                    Node::Fence(Box::new(Fence { open: Delim::None, close: Delim::None, body: Node::empty() })),
                ]),
                vec![1, 0],
            ),
            Template::Pile => (
                Node::Matrix(Box::new(Matrix {
                    rows: vec![vec![Node::empty()], vec![Node::empty()]],
                    delims: None,
                    kind: MatrixKind::Pile,
                    align: Side::Center,
                    columns: Vec::new(),
                    rules: false,
                })),
                vec![0],
            ),
        }
    }

    /// Command name used in menus and tests.
    pub fn name(self) -> &'static str {
        match self {
            Template::Fraction => "fraction",
            Template::SlashFraction => "slashed fraction",
            Template::Binomial => "binomial",
            Template::SquareRoot => "square root",
            Template::NthRoot => "nth root",
            Template::Superscript => "superscript",
            Template::Subscript => "subscript",
            Template::SubSuperscript => "sub and superscript",
            Template::Sum => "summation",
            Template::Product => "product",
            Template::Coproduct => "coproduct",
            Template::Integral => "integral",
            Template::DoubleIntegral => "double integral",
            Template::TripleIntegral => "triple integral",
            Template::ContourIntegral => "contour integral",
            Template::LimitBelow => "limit below",
            Template::LimitAbove => "limit above",
            Template::OverBar => "overbar",
            Template::UnderBar => "underbar",
            Template::OverBrace => "overbrace",
            Template::UnderBrace => "underbrace",
            Template::OverArrow => "over arrow",
            Template::UnderArrow => "under arrow",
            Template::Fence(..) => "fence",
            Template::Matrix { .. } => "matrix",
            Template::Accent(_) => "accent",
            Template::Function(_) => "function",
            Template::Pile => "pile",
        }
    }
}

fn big_op(op: char, limits: bool) -> Node {
    Node::Big(Box::new(BigOp {
        base: Node::row([Node::symbol(op, Class::Op)]),
        sub: Some(Node::empty()),
        sup: Some(Node::empty()),
        limits,
    }))
}

fn accent(kind: AccentKind, under: bool) -> Node {
    Node::Accent(Box::new(Accent { kind, base: Node::empty(), under }))
}

fn fence(open: Delim, close: Delim) -> Node {
    Node::Fence(Box::new(Fence { open, close, body: Node::empty() }))
}

fn default_alignment(kind: MatrixKind) -> Side {
    match kind {
        MatrixKind::Cases | MatrixKind::Aligned => Side::Left,
        _ => Side::Center,
    }
}

/// Editor state: the equation, the caret, an optional selection anchor and the undo history.
#[derive(Clone, Debug)]
pub struct Editor {
    math: Math,
    cursor: Cursor,
    anchor: Option<Cursor>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Font applied to characters typed next, as chosen in the toolbar.
    typing_font: Font,
    /// Size style wrapped around the next node inserted at the caret, as chosen in the toolbar.
    typing_style: Option<MathStyle>,
}

#[derive(Clone, Debug)]
struct Snapshot {
    math: Math,
    cursor: Cursor,
    anchor: Option<Cursor>,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    /// Empty equation, caret at the start.
    pub fn new() -> Self {
        Self {
            math: Math::new(Node::empty()),
            cursor: Cursor::start(),
            anchor: None,
            undo: Vec::new(),
            redo: Vec::new(),
            typing_font: Font::Italic,
            typing_style: None,
        }
    }

    /// Load an existing equation and put the caret at its start.
    pub fn from_math(mut math: Math) -> Self {
        if !math.body.is_row() {
            math.body = Node::row([math.body]);
        }
        math.body.normalize();
        Self { math, ..Self::new() }
    }

    pub fn math(&self) -> &Math {
        &self.math
    }

    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    pub fn typing_font(&self) -> Font {
        self.typing_font
    }

    /// Choose the font used for characters typed next.
    pub fn set_typing_font(&mut self, font: Font) {
        self.typing_font = font;
    }

    /// Size preset recorded for the next insertion, if any.
    pub fn typing_style(&self) -> Option<MathStyle> {
        self.typing_style
    }

    /// The row the caret sits in, if the caret is still valid.
    pub fn row(&self) -> Option<&Row> {
        match self.math.node_at(&self.cursor.path) {
            Some(Node::Row(row)) => Some(row),
            _ => None,
        }
    }

    /// Selected index range inside the caret's row, when a selection exists.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor.as_ref()?;
        if anchor.path != self.cursor.path {
            return None;
        }
        let (start, end) = (anchor.index.min(self.cursor.index), anchor.index.max(self.cursor.index));
        (start != end).then_some((start, end))
    }

    /// Drop the selection anchor.
    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// Place the caret, for click-to-position and other non-keyboard navigation.
    pub fn set_cursor(&mut self, cursor: Cursor) {
        let valid = matches!(self.math.node_at(&cursor.path), Some(Node::Row(row)) if cursor.index <= row.len());
        if valid {
            self.cursor = cursor;
            self.anchor = None;
        }
    }

    /// Place the caret and extend the selection from where it already was.
    pub fn extend_to(&mut self, cursor: Cursor) {
        let valid = matches!(self.math.node_at(&cursor.path), Some(Node::Row(row)) if cursor.index <= row.len());
        if valid {
            self.select_to(cursor);
        }
    }

    /// Extend the selection to the current caret.
    pub fn select_to(&mut self, cursor: Cursor) {
        if self.anchor.is_none() {
            self.anchor = Some(self.cursor.clone());
        }
        self.cursor = cursor;
    }

    // ---- mutation ----------------------------------------------------------------------

    /// Insert a character, classifying it the way an author expects from a keyboard.
    pub fn insert_char(&mut self, ch: char) -> bool {
        let node = classify(ch, self.typing_font);
        let Some(node) = node else {
            return false;
        };
        self.insert_node(node);
        true
    }

    /// Insert a character with an explicit class and font, as palette symbols do.
    pub fn insert_symbol(&mut self, ch: char, class: Class, font: Font) {
        self.insert_node(Node::Char(Char { ch, class, font }));
    }

    /// Insert literal upright text, e.g. a function name.
    pub fn insert_text(&mut self, text: &str) {
        let node = Node::Text(Box::new(TextRun { text: text.to_owned(), font: Font::Upright }));
        self.insert_node(node);
    }

    /// Insert an explicit space of the given kind.
    pub fn insert_space(&mut self, space: Space) {
        self.insert_node(Node::Space(space));
    }

    /// Insert a node at the caret and move past it.
    pub fn insert_node(&mut self, node: Node) {
        let node = match self.typing_style {
            Some(style) => Node::Styled(Box::new(Styled {
                body: node,
                style: Some(style),
                font: None,
                class: None,
            })),
            None => node,
        };
        self.push_undo();
        self.delete_selection_in_place();
        let index = self.cursor.index;
        if let Some(Node::Row(row)) = self.math.node_at_mut(&self.cursor.path) {
            let index = index.min(row.len());
            row.insert(index, node);
            self.cursor.index = index + 1;
            self.anchor = None;
        }
    }

    /// Insert a template, put the caret in its entry slot and select the old selection into it.
    pub fn insert_template(&mut self, template: Template) {
        self.push_undo();
        let selected = self.take_selection();
        let (node, entry) = template.build();
        let index = self.cursor.index;
        if let Some(Node::Row(row)) = self.math.node_at_mut(&self.cursor.path) {
            let index = index.min(row.len());
            row.insert(index, node);
            let node_path = self.cursor.path.child(index);
            let mut target = node_path;
            for slot in &entry {
                target = target.child(*slot);
            }
            if let Some(selected) = selected {
                if !entry.is_empty() {
                    // Walk the full entry path so nested templates keep their surrounding slots.
                    if let Some(slot) = self.math.node_at_mut(&target) {
                        *slot = Node::Row(selected);
                    }
                }
            }
            self.cursor = Cursor { path: target, index: 0 };
            self.anchor = None;
        }
    }

    /// Apply a template around the current selection, keeping the selection as its content.
    pub fn wrap_with(&mut self, template: Template) {
        self.insert_template(template);
    }

    /// Typing that replaces the selection, returning the removed nodes.
    fn take_selection(&mut self) -> Option<Vec<Node>> {
        let (start, end) = self.selection()?;
        let path = self.cursor.path.clone();
        let removed = match self.math.node_at_mut(&path) {
            Some(Node::Row(row)) => row.drain(start..end).collect::<Vec<_>>(),
            _ => return None,
        };
        self.cursor = self.cursor.at(start);
        self.anchor = None;
        Some(removed)
    }

    fn delete_selection_in_place(&mut self) {
        if let Some((start, end)) = self.selection() {
            if let Some(Node::Row(row)) = self.math.node_at_mut(&self.cursor.path) {
                row.drain(start..end);
                self.cursor.index = start;
            }
            self.anchor = None;
        }
    }

    /// Delete the item before the caret, or leave an empty template behind.
    pub fn backspace(&mut self) -> bool {
        if self.selection().is_some() {
            self.push_undo();
            self.delete_selection_in_place();
            return true;
        }
        if self.cursor.index > 0 {
            self.push_undo();
            if let Some(Node::Row(row)) = self.math.node_at_mut(&self.cursor.path) {
                let index = self.cursor.index - 1;
                if index < row.len() {
                    row.remove(index);
                    self.cursor.index = index;
                    return true;
                }
            }
            return false;
        }
        self.leave_empty_template()
    }

    /// Delete the item after the caret.
    pub fn delete(&mut self) -> bool {
        if self.selection().is_some() {
            self.push_undo();
            self.delete_selection_in_place();
            return true;
        }
        self.push_undo();
        if let Some(Node::Row(row)) = self.math.node_at_mut(&self.cursor.path) {
            if self.cursor.index < row.len() {
                row.remove(self.cursor.index);
                return true;
            }
        }
        self.pop_undo();
        false
    }

    /// When the caret sits at the start of an empty slot, remove the owning template and put the
    /// caret where the template was. Reports whether anything was removed.
    fn leave_empty_template(&mut self) -> bool {
        let Some((owner_path, _slot)) = self.cursor.path.split_last() else {
            return false;
        };
        if !matches!(self.math.node_at(&self.cursor.path), Some(Node::Row(row)) if row.is_empty()) {
            return false;
        }
        // The owning template sits in the row named by dropping one more index.
        let Some((row_path, owner_index)) = owner_path.split_last() else {
            return false;
        };
        // A template is removable exactly when every slot it owns is still empty. Asking the node's
        // own slot interface keeps this in step with any template added to `mt-core` later; the
        // owner cannot be a plain row, which is not a template and is left in place.
        let removable = self.math.node_at(&owner_path).is_some_and(|owner| {
            !owner.is_row() && !owner.slots().is_empty() && owner.slots().iter().all(|slot| slot.is_empty_slot())
        });
        if !removable {
            return false;
        }
        self.push_undo();
        if let Some(Node::Row(row)) = self.math.node_at_mut(&row_path) {
            if owner_index < row.len() {
                row.remove(owner_index);
                self.cursor = Cursor { path: row_path, index: owner_index };
                self.anchor = None;
                return true;
            }
        }
        self.pop_undo();
        false
    }

    // ---- navigation --------------------------------------------------------------------

    /// Move one position left, entering the construct to the left or leaving the current slot.
    pub fn move_left(&mut self) {
        self.anchor = None;
        if self.cursor.index > 0 {
            let previous = self.cursor.index - 1;
            if self.node_at_index(previous).is_some_and(|node| node.slot_count() > 0) {
                let path = self.deepest_last_slot(&self.cursor.path.child(previous));
                // The deepest slot may end on a template node itself, not a row we can enter.
                if self.math.node_at(&path).is_some_and(Node::is_row) {
                    self.cursor = Cursor { path, index: 0 };
                    let len = self.row().map_or(0, Row::len);
                    self.cursor.index = len;
                } else {
                    self.cursor.index = previous;
                }
            } else {
                self.cursor.index = previous;
            }
            return;
        }
        if let Some((parent_path, slot)) = self.cursor.path.split_last() {
            self.cursor = Cursor { path: parent_path, index: slot };
        }
    }

    /// Move one position right, entering the construct to the right or leaving the current slot.
    pub fn move_right(&mut self) {
        self.anchor = None;
        let len = self.row().map_or(0, Row::len);
        if self.cursor.index < len {
            let next = self.cursor.index;
            if self.node_at_index(next).is_some_and(|node| node.slot_count() > 0) {
                let path = self.deepest_first_slot(&self.cursor.path.child(next));
                // The deepest slot may end on a template node itself, not a row we can enter.
                if self.math.node_at(&path).is_some_and(Node::is_row) {
                    self.cursor = Cursor { path, index: 0 };
                } else {
                    self.cursor.index = next + 1;
                }
            } else {
                self.cursor.index = next + 1;
            }
            return;
        }
        if let Some((parent_path, slot)) = self.cursor.path.split_last() {
            self.cursor = Cursor { path: parent_path, index: slot + 1 };
        }
    }

    fn node_at_index(&self, index: usize) -> Option<&Node> {
        self.row()?.get(index)
    }

    /// Deepest slot reachable by always taking the first slot, stopping at non-row slots.
    fn deepest_first_slot(&self, path: &Path) -> Path {
        let mut current = path.clone();
        while let Some(Node::Row(_)) = self.math.node_at(&current).and_then(|node| node.slot(0)) {
            current = current.child(0);
        }
        current
    }

    /// Deepest slot reachable by always taking the last slot, stopping at non-row slots.
    fn deepest_last_slot(&self, path: &Path) -> Path {
        let mut current = path.clone();
        while let Some(node) = self.math.node_at(&current) {
            let Some(last) = node.slot_count().checked_sub(1) else { break };
            match node.slot(last) {
                Some(Node::Row(_)) => current = current.child(last),
                _ => break,
            }
        }
        current
    }

    /// Move the caret to the start of the row.
    pub fn move_line_start(&mut self) {
        self.anchor = None;
        self.cursor.index = 0;
    }

    /// Move the caret past the last item of the row.
    pub fn move_line_end(&mut self) {
        self.anchor = None;
        let len = self.row().map_or(0, Row::len);
        self.cursor.index = len;
    }

    /// Move between the stacked parts of the construct the caret is inside.
    pub fn move_vertical(&mut self, up: bool) {
        self.anchor = None;
        let Some((parent_path, slot)) = self.cursor.path.split_last() else {
            return;
        };
        let target = match self.math.node_at(&parent_path) {
            Some(Node::Frac(_)) => Some(if slot == 0 { 1 } else { 0 }),
            Some(Node::Scripts(_)) | Some(Node::Big(_)) | Some(Node::Limits(_)) => {
                script_step(slot, up)
            }
            Some(Node::Sqrt(sqrt)) if sqrt.index.is_some() => Some(if slot == 0 { 1 } else { 0 }),
            Some(Node::Matrix(matrix)) => matrix_step(matrix, slot, up),
            _ => None,
        };
        let Some(target) = target else { return };
        let target_path = parent_path.child(target);
        // Single-script templates have a missing sibling slot; only move into an existing row.
        if !self.math.node_at(&target_path).is_some_and(Node::is_row) {
            return;
        }
        let index = self.cursor.index.min(self.row_len(&target_path));
        self.cursor = Cursor { path: target_path, index };
    }

    fn row_len(&self, path: &Path) -> usize {
        match self.math.node_at(path) {
            Some(Node::Row(row)) => row.len(),
            _ => 0,
        }
    }

    /// Move to the next slot row in document order, wrapping at the end.
    pub fn tab(&mut self, forward: bool) {
        self.anchor = None;
        let mut rows = Vec::new();
        collect_rows(&self.math, &Path::root(), &mut rows);
        if rows.is_empty() {
            return;
        }
        let position = rows.iter().position(|path| *path == self.cursor.path);
        let next = match position {
            Some(position) if forward => (position + 1) % rows.len(),
            Some(position) => (position + rows.len() - 1) % rows.len(),
            None => 0,
        };
        self.cursor = Cursor { path: rows[next].clone(), index: 0 };
    }

    // ---- styling -----------------------------------------------------------------------

    /// Apply a font to the current selection, or record it for the next keystroke.
    pub fn apply_font(&mut self, font: Font) {
        self.typing_font = font;
        let Some((start, end)) = self.selection() else {
            return;
        };
        self.push_undo();
        let path = self.cursor.path.clone();
        if let Some(Node::Row(row)) = self.math.node_at_mut(&path) {
            for node in row.iter_mut().take(end).skip(start) {
                restyle(node, Some(font), None);
            }
        }
    }

    /// Apply a size style to the current selection, or record it for the next insertion.
    pub fn apply_style(&mut self, style: MathStyle) {
        self.typing_style = Some(style);
        let Some((start, end)) = self.selection() else {
            return;
        };
        self.push_undo();
        let path = self.cursor.path.clone();
        if let Some(Node::Row(row)) = self.math.node_at_mut(&path) {
            for node in row.iter_mut().take(end).skip(start) {
                restyle(node, None, Some(style));
            }
        }
    }

    // ---- history -----------------------------------------------------------------------

    fn push_undo(&mut self) {
        self.undo.push(Snapshot {
            math: self.math.clone(),
            cursor: self.cursor.clone(),
            anchor: self.anchor.clone(),
        });
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn pop_undo(&mut self) {
        self.undo.pop();
    }

    /// Undo the last command, reporting whether anything changed.
    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        self.redo.push(Snapshot {
            math: self.math.clone(),
            cursor: self.cursor.clone(),
            anchor: self.anchor.clone(),
        });
        self.restore(snapshot);
        true
    }

    /// Redo the last undone command, reporting whether anything changed.
    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo.pop() else {
            return false;
        };
        self.undo.push(Snapshot {
            math: self.math.clone(),
            cursor: self.cursor.clone(),
            anchor: self.anchor.clone(),
        });
        self.restore(snapshot);
        true
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.math = snapshot.math;
        self.cursor = snapshot.cursor;
        self.anchor = snapshot.anchor;
        if self.math.node_at(&self.cursor.path).is_none() {
            self.cursor = Cursor::start();
        }
    }

    /// Replace the equation, resetting history.
    pub fn set_math(&mut self, math: Math) {
        *self = Self::from_math(math);
    }
}

fn script_step(slot: usize, up: bool) -> Option<usize> {
    match (slot, up) {
        (0, true) => Some(2),
        (0, false) => Some(1),
        (1, true) => Some(0),
        (1, false) => Some(1),
        (2, true) => Some(2),
        (2, false) => Some(0),
        _ => None,
    }
}

fn matrix_step(matrix: &Matrix, slot: usize, up: bool) -> Option<usize> {
    let cols = matrix.col_count();
    if cols == 0 {
        return None;
    }
    let (row, col) = (slot / cols, slot % cols);
    let target_row = if up { row.checked_sub(1)? } else { row + 1 };
    let target = target_row * cols + col;
    let cells: usize = matrix.rows.iter().map(Row::len).sum();
    (target < cells).then_some(target)
}

fn collect_rows(math: &Math, path: &Path, rows: &mut Vec<Path>) {
    let Some(node) = math.node_at(path) else {
        return;
    };
    if node.is_row() {
        rows.push(path.clone());
    }
    for slot in 0..node.slot_count() {
        collect_rows(math, &path.child(slot), rows);
    }
}

fn restyle(node: &mut Node, font: Option<Font>, style: Option<MathStyle>) {
    match node {
        Node::Char(Char { font: node_font, .. }) => {
            if let Some(font) = font {
                *node_font = font;
            }
            if let Some(style) = style {
                let body = std::mem::replace(node, Node::empty());
                *node = Node::Styled(Box::new(Styled { body, style: Some(style), font, class: None }));
            }
        }
        other => {
            if font.is_some() || style.is_some() {
                let body = std::mem::replace(other, Node::empty());
                *other = Node::Styled(Box::new(Styled { body, style, font, class: None }));
            }
        }
    }
}

/// Classify a typed character the way it should appear in an equation.
pub fn classify(ch: char, current_font: Font) -> Option<Node> {
    if ch.is_whitespace() {
        return Some(Node::Space(Space::Medium));
    }
    let class = match ch {
        '+' | '-' | '\u{2212}' | '\u{00B1}' | '\u{2213}' | '*' | '\u{00D7}' | '\u{00F7}' | '\u{22C5}' | '\u{2217}'
        | '\u{2229}' | '\u{222A}' => Class::Bin,
        '=' | '<' | '>' | '\u{2264}' | '\u{2265}' | '\u{2260}' | '\u{2248}' | '\u{2261}' | '\u{223C}' | '\u{221D}'
        | '\u{2282}' | '\u{2283}' | '\u{2286}' | '\u{2287}' | '\u{2208}' | '\u{2209}' => Class::Rel,
        '(' | '[' | '{' | '\u{2308}' | '\u{230A}' | '\u{27E8}' => Class::Open,
        ')' | ']' | '}' | '\u{2309}' | '\u{230B}' | '\u{27E9}' => Class::Close,
        ',' | ';' | ':' => Class::Punct,
        '\u{2211}' | '\u{220F}' | '\u{222B}' | '\u{222E}' | '\u{2210}' => Class::Op,
        _ => Class::Ord,
    };
    let font = if class == Class::Ord && ch.is_alphabetic() {
        current_font
    } else {
        Font::Upright
    };
    Some(Node::Char(Char { ch, class, font }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(editor: &Editor) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(Node::Row(row)) = editor.math().node_at(&Path::root()) {
            for node in row {
                match node {
                    Node::Char(Char { ch, .. }) => out.push(ch.to_string()),
                    Node::Text(run) => out.push(run.text.clone()),
                    other => out.push(format!("<{}>", other.slot_count())),
                }
            }
        }
        out
    }

    #[test]
    fn typing_classifies_characters() {
        let mut editor = Editor::new();
        for ch in "a+b=1".chars() {
            assert!(editor.insert_char(ch));
        }
        assert_eq!(flat(&editor), ["a", "+", "b", "=", "1"]);
        let Node::Row(row) = editor.math().node_at(&Path::root()).unwrap() else { unreachable!() };
        assert_eq!(row[1], Node::symbol('+', Class::Bin));
        assert_eq!(row[0], Node::variable('a'));
        assert_eq!(row[3], Node::symbol('=', Class::Rel));
    }

    #[test]
    fn whitespace_becomes_an_explicit_space() {
        let mut editor = Editor::new();
        editor.insert_char('a');
        editor.insert_char(' ');
        let Node::Row(row) = editor.math().node_at(&Path::root()).unwrap() else { unreachable!() };
        assert_eq!(row[1], Node::Space(Space::Medium));
    }

    #[test]
    fn fraction_moves_caret_into_the_numerator() {
        let mut editor = Editor::new();
        editor.insert_template(Template::Fraction);
        assert_eq!(editor.cursor().path.to_string(), "0.0");
        editor.insert_char('x');
        editor.move_vertical(false);
        assert_eq!(editor.cursor().path.to_string(), "0.1");
        editor.insert_char('y');
        let Node::Row(row) = editor.math().node_at(&Path::root()).unwrap() else { unreachable!() };
        assert_eq!(row.len(), 1);
        let Node::Frac(frac) = &row[0] else { panic!("expected a fraction, got {:?}", row[0]) };
        assert_eq!(frac.num, Node::row([Node::variable('x')]));
        assert_eq!(frac.den, Node::row([Node::variable('y')]));
    }

    #[test]
    fn nth_root_puts_the_caret_in_the_radicand() {
        let mut editor = Editor::new();
        editor.insert_template(Template::NthRoot);
        assert_eq!(editor.cursor().path.to_string(), "0.1");
        editor.insert_char('x');
        editor.move_vertical(true);
        assert_eq!(editor.cursor().path.to_string(), "0.0");
        editor.insert_char('3');
        let Node::Row(row) = editor.math().node_at(&Path::root()).unwrap() else { unreachable!() };
        let Node::Sqrt(sqrt) = &row[0] else { panic!("expected a radical") };
        assert_eq!(sqrt.radicand, Node::row([Node::variable('x')]));
        assert_eq!(sqrt.index.as_ref(), Some(&Node::row([Node::symbol('3', Class::Ord)])));
    }

    #[test]
    fn script_template_navigates_between_base_and_scripts() {
        let mut editor = Editor::new();
        editor.insert_template(Template::SubSuperscript);
        editor.insert_char('n');
        editor.move_vertical(false);
        assert_eq!(editor.cursor().path.to_string(), "0.1");
        editor.insert_char('i');
        editor.move_vertical(true);
        assert_eq!(editor.cursor().path.to_string(), "0.0");
        editor.move_vertical(true);
        assert_eq!(editor.cursor().path.to_string(), "0.2");
        editor.insert_char('2');
        let Node::Row(row) = editor.math().node_at(&Path::root()).unwrap() else { unreachable!() };
        let Node::Scripts(scripts) = &row[0] else { panic!("expected scripts") };
        assert_eq!(scripts.base, Node::row([Node::variable('n')]));
        assert_eq!(scripts.sub, Some(Node::row([Node::variable('i')])));
        assert_eq!(scripts.sup, Some(Node::row([Node::symbol('2', Class::Ord)])));
    }

    #[test]
    fn inserting_an_empty_template_then_leaving_removes_it() {
        let mut editor = Editor::new();
        editor.insert_template(Template::Fraction);
        assert_eq!(editor.math().body.slot_count(), 1);
        assert!(editor.backspace());
        assert_eq!(editor.math().body.slot_count(), 0);
        assert_eq!(editor.cursor().path.to_string(), ".");
    }

    #[test]
    fn a_recorded_style_wraps_the_next_inserted_node() {
        let mut editor = Editor::new();
        editor.apply_style(MathStyle::Script);
        editor.insert_char('x');
        let Node::Row(row) = editor.math().node_at(&Path::root()).unwrap() else { unreachable!() };
        assert_eq!(row.len(), 1);
        let Node::Styled(styled) = &row[0] else { panic!("expected a styled node, got {:?}", row[0]) };
        assert_eq!(styled.style, Some(MathStyle::Script));
        assert_eq!(styled.body, Node::variable('x'));
    }

    #[test]
    fn backspace_keeps_a_matrix_holding_one_filled_cell() {
        let mut editor = Editor::new();
        editor.insert_template(Template::Matrix {
            rows: 2,
            cols: 2,
            kind: MatrixKind::Matrix,
            delims: None,
        });
        editor.insert_char('a');
        // Park the caret in a still-empty cell and try to leave the template.
        editor.cursor = Cursor { path: Path::root().child(0).child(1), index: 0 };
        assert!(!editor.backspace(), "a matrix with a filled cell must not be deleted");
        let Node::Row(row) = editor.math().node_at(&Path::root()).unwrap() else { unreachable!() };
        assert_eq!(row.len(), 1);
        let Node::Matrix(matrix) = &row[0] else { panic!("expected a matrix, got {:?}", row[0]) };
        assert_eq!(matrix.rows[0][0], Node::row([Node::variable('a')]));
    }

    #[test]
    fn backspace_keeps_a_partly_filled_template() {
        let mut editor = Editor::new();
        editor.insert_template(Template::Fraction);
        editor.insert_char('x');
        editor.move_vertical(false);
        editor.backspace();
        assert_eq!(editor.math().body.slot_count(), 1);
        assert!(matches!(editor.math().body.slot(0), Some(Node::Frac(_))));
    }

    #[test]
    fn selection_wraps_into_a_template() {
        let mut editor = Editor::new();
        for ch in "ab".chars() {
            editor.insert_char(ch);
        }
        let start = Cursor { path: Path::root(), index: 0 };
        editor.cursor = start.clone();
        editor.anchor = Some(start);
        editor.cursor = Cursor { path: Path::root(), index: 2 };
        assert_eq!(editor.selection(), Some((0, 2)));
        editor.wrap_with(Template::SquareRoot);
        let Node::Row(row) = editor.math().node_at(&Path::root()).unwrap() else { unreachable!() };
        assert_eq!(row.len(), 1);
        let Node::Sqrt(sqrt) = &row[0] else { panic!("expected a radical") };
        assert_eq!(sqrt.radicand, Node::row([Node::variable('a'), Node::variable('b')]));
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut editor = Editor::new();
        for ch in "abc".chars() {
            editor.insert_char(ch);
        }
        editor.cursor = Cursor { path: Path::root(), index: 1 };
        editor.anchor = Some(Cursor { path: Path::root(), index: 0 });
        editor.insert_char('z');
        assert_eq!(flat(&editor), ["z", "b", "c"]);
    }

    #[test]
    fn arrow_keys_cross_slot_boundaries() {
        fn caret(editor: &Editor) -> (String, usize) {
            (editor.cursor().path.to_string(), editor.cursor().index)
        }

        let mut editor = Editor::new();
        editor.insert_char('a');
        editor.insert_template(Template::SquareRoot);
        editor.insert_char('x');
        assert_eq!(caret(&editor), ("1.0".to_owned(), 1));
        editor.move_left();
        assert_eq!(caret(&editor), ("1.0".to_owned(), 0));
        editor.move_left();
        assert_eq!(caret(&editor), ("1".to_owned(), 0));
        editor.move_left();
        assert_eq!(caret(&editor), (".".to_owned(), 1));
        editor.move_right();
        assert_eq!(caret(&editor), ("1.0".to_owned(), 0));
    }

    #[test]
    fn tab_visits_rows_in_document_order() {
        let mut editor = Editor::new();
        editor.insert_template(Template::Fraction);
        let visited = {
            let mut seen = vec![editor.cursor().path.to_string()];
            for _ in 0..4 {
                editor.tab(true);
                seen.push(editor.cursor().path.to_string());
            }
            seen
        };
        assert_eq!(visited, ["0.0", "0.1", ".", "0.0", "0.1"]);
        editor.tab(false);
        assert_eq!(editor.cursor().path.to_string(), "0.0");
    }

    #[test]
    fn undo_and_redo_restore_equation_and_caret() {
        let mut editor = Editor::new();
        editor.insert_char('a');
        editor.insert_template(Template::Fraction);
        editor.insert_char('1');
        assert_eq!(editor.cursor().path.to_string(), "1.0");
        assert!(editor.undo());
        assert_eq!(flat(&editor), ["a", "<2>"]);
        assert!(editor.undo());
        assert_eq!(flat(&editor), ["a"]);
        assert!(editor.redo());
        assert_eq!(flat(&editor), ["a", "<2>"]);
        assert_eq!(editor.cursor().path.to_string(), "1.0");
    }

    #[test]
    fn applying_a_font_restyles_the_selection() {
        let mut editor = Editor::new();
        for ch in "ab".chars() {
            editor.insert_char(ch);
        }
        editor.cursor = Cursor { path: Path::root(), index: 2 };
        editor.anchor = Some(Cursor { path: Path::root(), index: 0 });
        editor.apply_font(Font::BoldItalic);
        let Node::Row(row) = editor.math().node_at(&Path::root()).unwrap() else { unreachable!() };
        assert_eq!(row[0], Node::char_with('a', Class::Ord, Font::BoldItalic));
        assert_eq!(row[1], Node::char_with('b', Class::Ord, Font::BoldItalic));
        assert_eq!(editor.typing_font(), Font::BoldItalic);
    }

    #[test]
    fn matrix_navigation_moves_between_rows() {
        let mut editor = Editor::new();
        editor.insert_template(Template::Matrix {
            rows: 2,
            cols: 2,
            kind: MatrixKind::Matrix,
            delims: Some((Delim::Paren, Delim::Paren)),
        });
        assert_eq!(editor.cursor().path.to_string(), "0.0");
        editor.move_vertical(false);
        assert_eq!(editor.cursor().path.to_string(), "0.2");
        editor.move_vertical(false);
        assert_eq!(editor.cursor().path.to_string(), "0.2");
        editor.move_vertical(true);
        assert_eq!(editor.cursor().path.to_string(), "0.0");
    }

    #[test]
    fn vertical_move_stays_put_when_the_target_slot_does_not_exist() {
        let mut editor = Editor::new();
        editor.insert_template(Template::Superscript);
        // Park the caret in the base and try to move down into the missing subscript.
        editor.cursor = Cursor { path: Path::root().child(0).child(0), index: 0 };
        editor.move_vertical(false);
        assert_eq!(editor.cursor().path.to_string(), "0.0");
        editor.insert_char('x');
        let Node::Row(row) = editor.math().node_at(&Path::root()).unwrap() else { unreachable!() };
        let Node::Scripts(scripts) = &row[0] else { panic!("expected scripts") };
        assert_eq!(scripts.base, Node::row([Node::variable('x')]));
        assert_eq!(scripts.sub, None);
    }

    #[test]
    fn move_left_falls_back_when_the_deepest_slot_is_not_a_row() {
        let mut editor = Editor::new();
        editor.insert_char('a');
        editor.insert_template(Template::LimitBelow);
        editor.insert_char('x');
        editor.move_right();
        editor.move_right();
        editor.insert_char('b');
        // Moving left twice: first past 'b', then onto the limit below; its last slot (over)
        // is missing, so the caret must fall back to the position before the template instead
        // of a dead path.
        editor.move_left();
        editor.move_left();
        assert_eq!(editor.cursor().path.to_string(), ".");
        assert_eq!(editor.cursor().index, 1);
        editor.insert_char('z');
        assert_eq!(flat(&editor), ["a", "z", "<3>", "b"]);
    }

    #[test]
    fn move_right_falls_back_when_the_deepest_slot_is_not_a_row() {
        let mut editor = Editor::new();
        editor.insert_char('a');
        editor.apply_style(MathStyle::Script);
        editor.insert_char('x');
        editor.cursor = Cursor { path: Path::root(), index: 0 };
        // Moving right onto the styled character must not park the caret on the Styled node.
        editor.move_right();
        assert_eq!(editor.cursor().path.to_string(), ".");
        assert_eq!(editor.cursor().index, 1);
        editor.move_right();
        assert_eq!(editor.cursor().path.to_string(), ".");
        assert_eq!(editor.cursor().index, 2);
        editor.typing_style = None;
        editor.insert_char('y');
        assert_eq!(flat(&editor), ["a", "<1>", "y"]);
    }

    #[test]
    fn a_selection_transfers_into_a_function_body_keeping_the_fence() {
        let mut editor = Editor::new();
        for ch in "ab".chars() {
            editor.insert_char(ch);
        }
        editor.cursor = Cursor { path: Path::root(), index: 2 };
        editor.anchor = Some(Cursor { path: Path::root(), index: 0 });
        editor.wrap_with(Template::Function("sin"));
        let Node::Row(row) = editor.math().node_at(&Path::root()).unwrap() else { unreachable!() };
        assert_eq!(row.len(), 1);
        let Node::Fence(fence) = row[0].slot(1).unwrap() else { panic!("expected a fence, got {:?}", row[0]) };
        assert_eq!(fence.body, Node::row([Node::variable('a'), Node::variable('b')]));
        assert_eq!(editor.cursor().path.to_string(), "0.1.0");
    }

    #[test]
    fn applying_a_style_wraps_plain_characters() {
        let mut editor = Editor::new();
        editor.insert_char('x');
        editor.cursor = Cursor { path: Path::root(), index: 1 };
        editor.anchor = Some(Cursor { path: Path::root(), index: 0 });
        editor.apply_style(MathStyle::Script);
        let Node::Row(row) = editor.math().node_at(&Path::root()).unwrap() else { unreachable!() };
        let Node::Styled(styled) = &row[0] else { panic!("expected a styled node, got {:?}", row[0]) };
        assert_eq!(styled.style, Some(MathStyle::Script));
        assert_eq!(styled.body, Node::variable('x'));
    }
}
