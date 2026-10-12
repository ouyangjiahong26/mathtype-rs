//! The equation tree shared by the parsers, the editor and the renderers.
//!
//! The model follows the shape of an equation as an author sees it rather than a TeX token
//! stream: an equation is a [`Row`] of items, templates such as fractions or radicals own their
//! argument slots, and every node exposes a fixed, ordered list of slots. Editing code can
//! therefore walk and mutate any construct without knowing what the construct is, and layout can
//! recurse over the same slot list.

/// A complete equation together with the size preset it is laid out at.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Math {
    pub body: Node,
    pub style: MathStyle,
}

impl Math {
    /// An equation wrapping `body` at full size.
    pub fn new(body: impl Into<Node>) -> Self {
        Self {
            body: body.into(),
            style: MathStyle::Full,
        }
    }

    pub fn node_at(&self, path: &Path) -> Option<&Node> {
        self.body.node_at(path)
    }

    pub fn node_at_mut(&mut self, path: &Path) -> Option<&mut Node> {
        self.body.node_at_mut(path)
    }
}

/// Size preset an equation is laid out at, matching the size classes an author picks from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MathStyle {
    /// Inline text size.
    Text,
    /// Smaller than text, one step down in the nesting hierarchy.
    Script,
    /// Smallest supported size, two steps down in the nesting hierarchy.
    ScriptScript,
    /// Full size.
    #[default]
    Full,
}

impl MathStyle {
    /// The next smaller preset, saturating at [`MathStyle::ScriptScript`].
    pub fn smaller(self) -> Self {
        match self {
            MathStyle::Full => MathStyle::Script,
            MathStyle::Text => MathStyle::Script,
            MathStyle::Script => MathStyle::ScriptScript,
            MathStyle::ScriptScript => MathStyle::ScriptScript,
        }
    }
}

/// Horizontal sequence of items.
pub type Row = Vec<Node>;

/// One item of an equation.
///
/// Template variants are boxed so that a `Node` stays pointer sized.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    /// Horizontal sequence of items; an empty row is an empty argument slot.
    Row(Row),
    /// A single character with its atom class and font.
    Char(Char),
    /// Literal text set upright (`\text{...}` and friends).
    Text(Box<TextRun>),
    /// Explicit horizontal spacing.
    Space(Space),
    /// Fraction, with or without a rule.
    Frac(Box<Frac>),
    /// Radical with an optional index.
    Sqrt(Box<Sqrt>),
    /// Subscript and/or superscript attached to a base.
    Scripts(Box<Scripts>),
    /// Over/under material that is not an accent (`\overset`, `\stackrel`, limits).
    Limits(Box<Limits>),
    /// Delimiters around a body.
    Fence(Box<Fence>),
    /// Table of cells with optional delimiters and alignment.
    Matrix(Box<Matrix>),
    /// Accent over or under a base.
    Accent(Box<Accent>),
    /// Large operator with optional limits.
    Big(Box<BigOp>),
    /// Style, font or class applied to a subtree.
    Styled(Box<Styled>),
    /// Alignment point (`&` inside aligned environments).
    Align,
    /// Construct kept verbatim because it could not be represented.
    Error(String),
}

impl Node {
    /// An empty row, i.e. an empty slot.
    pub fn empty() -> Self {
        Node::Row(Row::new())
    }

    pub fn row(items: impl IntoIterator<Item = Node>) -> Self {
        Node::Row(items.into_iter().collect())
    }

    /// A character carrying `class` and `font`.
    pub fn char_with(ch: char, class: Class, font: Font) -> Self {
        Node::Char(Char { ch, class, font })
    }

    /// A default-class italic variable.
    pub fn variable(ch: char) -> Self {
        Node::Char(Char {
            ch,
            class: Class::Ord,
            font: Font::Italic,
        })
    }

    /// A default-class upright operator or symbol.
    pub fn symbol(ch: char, class: Class) -> Self {
        Node::Char(Char {
            ch,
            class,
            font: Font::Upright,
        })
    }

    pub fn text(text: impl Into<String>) -> Self {
        Node::Text(Box::new(TextRun {
            text: text.into(),
            font: Font::Upright,
        }))
    }

    /// True when the node is an empty row, which renderers draw as an empty slot.
    pub fn is_empty_slot(&self) -> bool {
        matches!(self, Node::Row(row) if row.is_empty())
    }

    /// Whether the node is a row and can therefore accept siblings.
    pub fn is_row(&self) -> bool {
        matches!(self, Node::Row(_))
    }

    /// Number of child slots the node exposes, in visual order.
    pub fn slot_count(&self) -> usize {
        match self {
            Node::Row(row) => row.len(),
            Node::Char(_) | Node::Text(_) | Node::Space(_) | Node::Align | Node::Error(_) => 0,
            Node::Frac(_) => 2,
            Node::Sqrt(sqrt) => 1 + usize::from(sqrt.index.is_some()),
            Node::Scripts(_) => 3,
            Node::Limits(_) => 3,
            Node::Fence(_) => 1,
            Node::Matrix(matrix) => matrix.rows.iter().map(Row::len).sum(),
            Node::Accent(_) => 1,
            Node::Big(_) => 3,
            Node::Styled(_) => 1,
        }
    }

    /// Read-only access to slot `index`, in the order defined by [`Node::slot_count`].
    pub fn slot(&self, index: usize) -> Option<&Node> {
        match self {
            Node::Row(row) => row.get(index),
            Node::Char(_) | Node::Text(_) | Node::Space(_) | Node::Align | Node::Error(_) => None,
            Node::Frac(frac) => match index {
                0 => Some(&frac.num),
                1 => Some(&frac.den),
                _ => None,
            },
            Node::Sqrt(sqrt) => match (index, &sqrt.index) {
                (0, Some(radicand)) => Some(radicand),
                (0, None) => Some(&sqrt.radicand),
                (1, Some(_)) => Some(&sqrt.radicand),
                _ => None,
            },
            Node::Scripts(scripts) => match index {
                0 => Some(&scripts.base),
                1 => scripts.sub.as_ref(),
                2 => scripts.sup.as_ref(),
                _ => None,
            },
            Node::Limits(limits) => match index {
                0 => Some(&limits.base),
                1 => limits.under.as_ref(),
                2 => limits.over.as_ref(),
                _ => None,
            },
            Node::Fence(fence) => (index == 0).then_some(&fence.body),
            Node::Matrix(matrix) => matrix.cell_at(index),
            Node::Accent(accent) => (index == 0).then_some(&accent.base),
            Node::Big(big) => match index {
                0 => Some(&big.base),
                1 => big.sub.as_ref(),
                2 => big.sup.as_ref(),
                _ => None,
            },
            Node::Styled(styled) => (index == 0).then_some(&styled.body),
        }
    }

    /// Mutable access to slot `index`, in the order defined by [`Node::slot_count`].
    pub fn slot_mut(&mut self, index: usize) -> Option<&mut Node> {
        match self {
            Node::Row(row) => row.get_mut(index),
            Node::Char(_) | Node::Text(_) | Node::Space(_) | Node::Align | Node::Error(_) => None,
            Node::Frac(frac) => match index {
                0 => Some(&mut frac.num),
                1 => Some(&mut frac.den),
                _ => None,
            },
            Node::Sqrt(sqrt) => match (index, &mut sqrt.index) {
                (0, Some(radicand)) => Some(radicand),
                (0, None) => Some(&mut sqrt.radicand),
                (1, Some(_)) => Some(&mut sqrt.radicand),
                _ => None,
            },
            Node::Scripts(scripts) => match index {
                0 => Some(&mut scripts.base),
                1 => scripts.sub.as_mut(),
                2 => scripts.sup.as_mut(),
                _ => None,
            },
            Node::Limits(limits) => match index {
                0 => Some(&mut limits.base),
                1 => limits.under.as_mut(),
                2 => limits.over.as_mut(),
                _ => None,
            },
            Node::Fence(fence) => (index == 0).then_some(&mut fence.body),
            Node::Matrix(matrix) => matrix.cell_at_mut(index),
            Node::Accent(accent) => (index == 0).then_some(&mut accent.base),
            Node::Big(big) => match index {
                0 => Some(&mut big.base),
                1 => big.sub.as_mut(),
                2 => big.sup.as_mut(),
                _ => None,
            },
            Node::Styled(styled) => (index == 0).then_some(&mut styled.body),
        }
    }

    /// Slots in visual order. Allocates; use [`Node::slot_count`] plus [`Node::slot`] in hot paths.
    pub fn slots(&self) -> Vec<&Node> {
        (0..self.slot_count())
            .filter_map(|i| self.slot(i))
            .collect()
    }

    /// Depth-first walk over the node and every node reachable through its slots.
    pub fn walk<'a>(&'a self, visit: &mut impl FnMut(&'a Node)) {
        visit(self);
        for index in 0..self.slot_count() {
            if let Some(slot) = self.slot(index) {
                slot.walk(visit);
            }
        }
    }

    /// Node reached by following `path` from here.
    pub fn node_at(&self, path: &Path) -> Option<&Node> {
        let mut current = self;
        for &index in path.as_slice() {
            current = current.slot(index)?;
        }
        Some(current)
    }

    /// Mutable node reached by following `path` from here.
    pub fn node_at_mut(&mut self, path: &Path) -> Option<&mut Node> {
        let mut current = self;
        for &index in path.as_slice() {
            current = current.slot_mut(index)?;
        }
        Some(current)
    }

    /// Replace the node at `path`, returning the replaced node.
    ///
    /// The root cannot be replaced: pass a non-empty path.
    pub fn replace_at(&mut self, path: &Path, node: Node) -> Option<Node> {
        let (parent_path, index) = path.split_last()?;
        let parent = self.node_at_mut(&parent_path)?;
        match parent {
            Node::Row(row) => {
                let target = row.get_mut(index)?;
                Some(std::mem::replace(target, node))
            }
            other => {
                let target = other.slot_mut(index)?;
                Some(std::mem::replace(target, node))
            }
        }
    }

    /// Insert `node` into the row at `path`, directly before the item at `path` or appended when
    /// the path names a row itself.
    pub fn insert_at(&mut self, path: &Path, node: Node) -> bool {
        match self.node_at_mut(path) {
            Some(Node::Row(row)) => {
                row.push(node);
                true
            }
            Some(_) => match path.split_last() {
                Some((parent_path, index)) => match self.node_at_mut(&parent_path) {
                    Some(parent) => parent.insert_sibling(index, node),
                    None => false,
                },
                None => false,
            },
            None => false,
        }
    }

    /// Insert `node` before slot `index` of a row-like parent.
    fn insert_sibling(&mut self, index: usize, node: Node) -> bool {
        match self {
            Node::Row(row) => {
                let index = index.min(row.len());
                row.insert(index, node);
                true
            }
            _ => false,
        }
    }

    /// Remove the node at `path`, returning it. The root cannot be removed.
    pub fn remove_at(&mut self, path: &Path) -> Option<Node> {
        let (parent_path, index) = path.split_last()?;
        match self.node_at_mut(&parent_path)? {
            Node::Row(row) => (index < row.len()).then(|| row.remove(index)),
            _ => None,
        }
    }

    /// Flatten rows nested directly inside a row and drop empty nested rows.
    ///
    /// Template slots are untouched, so this only removes row nesting that carries no meaning and
    /// keeps parse-then-serialise round trips stable.
    pub fn normalize(&mut self) {
        for index in 0..self.slot_count() {
            if let Some(slot) = self.slot_mut(index) {
                slot.normalize();
            }
        }
        if let Node::Row(row) = self {
            let mut flat = Row::with_capacity(row.len());
            for node in row.drain(..) {
                match node {
                    Node::Row(inner) => flat.extend(inner),
                    other => flat.push(other),
                }
            }
            *row = flat;
        }
    }
}

impl From<char> for Node {
    fn from(ch: char) -> Self {
        Node::variable(ch)
    }
}

impl From<&str> for Node {
    fn from(text: &str) -> Self {
        Node::Text(Box::new(TextRun {
            text: text.to_owned(),
            font: Font::Upright,
        }))
    }
}

impl From<String> for Node {
    fn from(text: String) -> Self {
        Node::Text(Box::new(TextRun {
            text,
            font: Font::Upright,
        }))
    }
}

impl From<Vec<Node>> for Node {
    fn from(items: Vec<Node>) -> Self {
        Node::Row(items)
    }
}

impl Default for Node {
    /// An empty slot.
    fn default() -> Self {
        Node::empty()
    }
}

/// A character together with the atom class that drives spacing and the font it is set in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Char {
    pub ch: char,
    pub class: Class,
    pub font: Font,
}

/// Atom class, used for operator spacing between neighbouring atoms.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Class {
    /// Ordinary atom: variables, numbers, most symbols.
    #[default]
    Ord,
    /// Large operator or function name taking limits.
    Op,
    /// Binary operator.
    Bin,
    /// Relation.
    Rel,
    /// Opening delimiter.
    Open,
    /// Closing delimiter.
    Close,
    /// Punctuation.
    Punct,
    /// Inner atom, e.g. a fraction or matrix.
    Inner,
}

/// Typeface a run is set in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Font {
    /// Italic math variable, the default for letters.
    #[default]
    Italic,
    /// Upright roman.
    Upright,
    Bold,
    BoldItalic,
    SansSerif,
    SansSerifBold,
    SansSerifItalic,
    SansSerifBoldItalic,
    Monospace,
    Script,
    BoldScript,
    Fraktur,
    BoldFraktur,
    DoubleStruck,
    /// Symbols drawn from the symbol font rather than the text face.
    Symbol,
    /// Glyphs from the extra symbol face.
    Extra,
}

impl Font {
    /// Whether the face is set in italics, which layout needs for metric selection.
    pub fn is_italic(self) -> bool {
        matches!(
            self,
            Font::Italic | Font::BoldItalic | Font::SansSerifItalic | Font::SansSerifBoldItalic
        )
    }

    /// Whether the face is bold.
    pub fn is_bold(self) -> bool {
        matches!(
            self,
            Font::Bold
                | Font::BoldItalic
                | Font::SansSerifBold
                | Font::SansSerifBoldItalic
                | Font::BoldScript
                | Font::BoldFraktur
        )
    }
}

/// Literal text set upright, optionally in a specific face.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRun {
    pub text: String,
    pub font: Font,
}

/// Explicit horizontal space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Space {
    /// 3/18 em.
    Thin,
    /// 4/18 em.
    Medium,
    /// 5/18 em.
    Thick,
    /// 6/18 em.
    Sixteenth,
    /// 1 em.
    Quad,
    /// 2 em.
    DoubleQuad,
    /// -3/18 em.
    NegativeThin,
    /// -4/18 em.
    NegativeMedium,
    /// -5/18 em.
    NegativeThick,
}

impl Space {
    /// Width in em units, negative for backspacing.
    pub fn em(self) -> f64 {
        match self {
            Space::Thin => 3.0 / 18.0,
            Space::Medium => 4.0 / 18.0,
            Space::Thick => 5.0 / 18.0,
            Space::Sixteenth => 6.0 / 18.0,
            Space::Quad => 1.0,
            Space::DoubleQuad => 2.0,
            Space::NegativeThin => -3.0 / 18.0,
            Space::NegativeMedium => -4.0 / 18.0,
            Space::NegativeThick => -5.0 / 18.0,
        }
    }
}

/// Fraction, with or without a rule, drawn stacked or inline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frac {
    pub num: Node,
    pub den: Node,
    pub kind: FracKind,
}

impl Frac {
    pub fn new(num: Node, den: Node) -> Self {
        Self {
            num,
            den,
            kind: FracKind::Bar,
        }
    }
}

/// How a fraction is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FracKind {
    /// Stacked with a horizontal rule.
    #[default]
    Bar,
    /// Stacked with a slash instead of a rule.
    Slash,
    /// Stacked without a rule, e.g. binomial coefficients.
    NoBar,
    /// Written inline as `a/b`.
    Linear,
}

/// Radical, with an optional index for higher roots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sqrt {
    /// Root index; `None` is a square root.
    pub index: Option<Node>,
    pub radicand: Node,
}

impl Sqrt {
    pub fn square(radicand: Node) -> Self {
        Self {
            index: None,
            radicand,
        }
    }

    pub fn nth(index: Node, radicand: Node) -> Self {
        Self {
            index: Some(index),
            radicand,
        }
    }
}

/// Subscript and superscript attached to a base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scripts {
    pub base: Node,
    pub sub: Option<Node>,
    pub sup: Option<Node>,
}

impl Scripts {
    /// Attach `sub`/`sup` to `base`.
    ///
    /// Merging into an existing [`Node::Scripts`] node is up to the caller, which knows whether
    /// the new scripts belong inside or outside the existing ones.
    pub fn attach(base: Node, sub: Option<Node>, sup: Option<Node>) -> Self {
        Self { base, sub, sup }
    }
}

/// Over/under material that is not an accent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    pub base: Node,
    pub under: Option<Node>,
    pub over: Option<Node>,
    pub align: Side,
}

/// Horizontal alignment of over/under material.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Side {
    #[default]
    Center,
    Left,
    Right,
}

/// Delimiter shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Delim {
    Paren,
    Bracket,
    Brace,
    Abs,
    Norm,
    Angle,
    Floor,
    Ceil,
    /// No delimiter drawn.
    None,
    /// A specific character, drawn from a font that provides it.
    Custom(char),
    /// An arrow delimiter.
    Arrow,
    /// A double bar delimiter.
    DoubleBar,
}

impl Delim {
    /// The opening character, or `None` when nothing is drawn.
    pub fn open(self) -> Option<char> {
        Some(match self {
            Delim::Paren => '(',
            Delim::Bracket => '[',
            Delim::Brace => '{',
            Delim::Abs => '|',
            Delim::Norm => '\u{2016}',
            Delim::Angle => '\u{27E8}',
            Delim::Floor => '\u{230A}',
            Delim::Ceil => '\u{2308}',
            Delim::None => return None,
            Delim::Custom(ch) => ch,
            Delim::Arrow => '\u{27E8}',
            Delim::DoubleBar => '\u{2016}',
        })
    }

    /// The closing character, or `None` when nothing is drawn.
    pub fn close(self) -> Option<char> {
        Some(match self {
            Delim::Paren => ')',
            Delim::Bracket => ']',
            Delim::Brace => '}',
            Delim::Abs => '|',
            Delim::Norm => '\u{2016}',
            Delim::Angle => '\u{27E9}',
            Delim::Floor => '\u{230B}',
            Delim::Ceil => '\u{2309}',
            Delim::None => return None,
            Delim::Custom(ch) => ch,
            Delim::Arrow => '\u{27E9}',
            Delim::DoubleBar => '\u{2016}',
        })
    }
}

/// Delimiters around a body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fence {
    pub open: Delim,
    pub close: Delim,
    pub body: Node,
}

/// Table of cells with optional delimiters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Matrix {
    pub rows: Vec<Row>,
    pub delims: Option<(Delim, Delim)>,
    pub kind: MatrixKind,
    /// Alignment applied to every column; per-column rules come from [`Matrix::columns`].
    pub align: Side,
    /// Optional per-column alignment overriding [`Matrix::align`].
    pub columns: Vec<Side>,
    /// Draws the horizontal and vertical cell rules of an array.
    pub rules: bool,
}

impl Matrix {
    /// A matrix of `rows` rows and `cols` columns, each cell an empty slot.
    pub fn empty(rows: usize, cols: usize) -> Self {
        Self {
            rows: vec![vec![Node::empty(); cols]; rows],
            delims: None,
            kind: MatrixKind::Matrix,
            align: Side::Center,
            columns: Vec::new(),
            rules: false,
        }
    }

    /// Number of rows.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Number of columns, taken from the widest row.
    pub fn col_count(&self) -> usize {
        self.rows.iter().map(Row::len).max().unwrap_or(0)
    }

    fn cell_position(&self, index: usize) -> Option<(usize, usize)> {
        let mut remaining = index;
        for (row, cells) in self.rows.iter().enumerate() {
            if remaining < cells.len() {
                return Some((row, remaining));
            }
            remaining -= cells.len();
        }
        None
    }

    /// Cell addressed by a flat slot index, row-major.
    pub fn cell_at(&self, index: usize) -> Option<&Node> {
        let (row, col) = self.cell_position(index)?;
        self.rows[row].get(col)
    }

    /// Mutable cell addressed by a flat slot index, row-major.
    pub fn cell_at_mut(&mut self, index: usize) -> Option<&mut Node> {
        let (row, col) = self.cell_position(index)?;
        self.rows[row].get_mut(col)
    }
}

/// Kind of table, which decides delimiter defaults and column alignment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MatrixKind {
    #[default]
    Matrix,
    /// Piecewise definitions: left brace, left aligned columns.
    Cases,
    /// Aligned equations on the alignment points.
    Aligned,
    /// Plain array.
    Array,
    /// Vertically stacked material.
    Pile,
    /// Long division layout.
    LongDivision,
}

/// Accent drawn over or under a base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accent {
    pub kind: AccentKind,
    pub base: Node,
    /// Draws the accent under the base instead of over it.
    pub under: bool,
}

/// Accent shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccentKind {
    Hat,
    WideHat,
    Tilde,
    WideTilde,
    Bar,
    Vector,
    Dot,
    DoubleDot,
    TripleDot,
    Breve,
    Check,
    Acute,
    Grave,
    Circle,
    OverBrace,
    UnderBrace,
    OverBracket,
    UnderBracket,
    OverParen,
    UnderParen,
    ArrowRight,
    ArrowLeft,
    Line,
    Strike,
}

impl AccentKind {
    /// The character used when the accent is drawn from a font rather than as a rule or path.
    pub fn glyph(self) -> Option<char> {
        Some(match self {
            AccentKind::Hat => '\u{02C6}',
            AccentKind::WideHat => '\u{0302}',
            AccentKind::Tilde => '~',
            AccentKind::WideTilde => '\u{0303}',
            AccentKind::Dot => '\u{02D9}',
            AccentKind::DoubleDot => '\u{00A8}',
            AccentKind::TripleDot => '\u{20DB}',
            AccentKind::Breve => '\u{02D8}',
            AccentKind::Check => '\u{02C7}',
            AccentKind::Acute => '\u{00B4}',
            AccentKind::Grave => '`',
            AccentKind::Circle => '\u{02DA}',
            AccentKind::ArrowRight => '\u{2192}',
            AccentKind::ArrowLeft => '\u{2190}',
            AccentKind::Bar | AccentKind::Vector | AccentKind::Line => return None,
            AccentKind::Strike => return None,
            AccentKind::OverBrace => '\u{23DE}',
            AccentKind::UnderBrace => '\u{23DF}',
            AccentKind::OverBracket => '\u{23B4}',
            AccentKind::UnderBracket => '\u{23B5}',
            AccentKind::OverParen => '\u{23DC}',
            AccentKind::UnderParen => '\u{23DD}',
        })
    }
}

/// Large operator with optional limits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigOp {
    pub base: Node,
    pub sub: Option<Node>,
    pub sup: Option<Node>,
    /// Draws the limits above and below instead of beside the operator.
    pub limits: bool,
}

/// Style, font or class override applied to a subtree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Styled {
    pub body: Node,
    pub style: Option<MathStyle>,
    pub font: Option<Font>,
    pub class: Option<Class>,
}

/// Position in the tree: the sequence of slot indices from the root.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Path(Vec<usize>);

impl Path {
    /// The path naming the root.
    pub fn root() -> Self {
        Self(Vec::new())
    }

    /// Path with `index` appended.
    pub fn child(&self, index: usize) -> Self {
        let mut path = self.clone();
        path.0.push(index);
        path
    }

    /// Path with the last index removed, or `None` at the root.
    pub fn parent(&self) -> Option<Self> {
        let mut path = self.clone();
        path.0.pop().map(|_| path)
    }

    /// Last index, i.e. the slot of the parent that holds this node.
    pub fn last(&self) -> Option<usize> {
        self.0.last().copied()
    }

    /// Split into the parent path and the final index.
    pub fn split_last(&self) -> Option<(Self, usize)> {
        let (index, rest) = self.0.split_last()?;
        Some((Self(rest.to_vec()), *index))
    }

    pub fn push(&mut self, index: usize) {
        self.0.push(index);
    }

    pub fn pop(&mut self) -> Option<usize> {
        self.0.pop()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn as_slice(&self) -> &[usize] {
        &self.0
    }
}

impl FromIterator<usize> for Path {
    fn from_iter<T: IntoIterator<Item = usize>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl std::fmt::Display for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_empty() {
            return f.write_str(".");
        }
        for (position, index) in self.0.iter().enumerate() {
            if position > 0 {
                f.write_str(".")?;
            }
            write!(f, "{index}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scripts(base: Node, sub: Node, sup: Node) -> Node {
        Node::Scripts(Box::new(Scripts {
            base,
            sub: Some(sub),
            sup: Some(sup),
        }))
    }

    #[test]
    fn fraction_slots_are_numerator_then_denominator() {
        let frac = Node::Frac(Box::new(Frac::new(
            Node::variable('a'),
            Node::variable('b'),
        )));
        assert_eq!(frac.slot_count(), 2);
        assert_eq!(frac.slot(0), Some(&Node::variable('a')));
        assert_eq!(frac.slot(1), Some(&Node::variable('b')));
        assert_eq!(frac.slot(2), None);
    }

    #[test]
    fn square_root_has_a_single_slot() {
        let root = Node::Sqrt(Box::new(Sqrt::square(Node::variable('x'))));
        assert_eq!(root.slot_count(), 1);
        assert_eq!(root.slot(0), Some(&Node::variable('x')));

        let nth = Node::Sqrt(Box::new(Sqrt::nth(
            Node::variable('n'),
            Node::variable('x'),
        )));
        assert_eq!(nth.slot_count(), 2);
        assert_eq!(nth.slot(0), Some(&Node::variable('n')));
        assert_eq!(nth.slot(1), Some(&Node::variable('x')));
    }

    #[test]
    fn path_navigation_reaches_nested_slots() {
        let math = Math::new(Node::row([
            Node::variable('x'),
            scripts(
                Node::variable('a'),
                Node::variable('i'),
                Node::variable('2'),
            ),
        ]));
        let sup = Path::root().child(1).child(2);
        assert_eq!(math.node_at(&sup), Some(&Node::variable('2')));
        assert_eq!(sup.to_string(), "1.2");
        assert_eq!(
            sup.parent().map(|path| path.to_string()),
            Some("1".to_owned())
        );
        assert_eq!(math.node_at(&Path::root()), Some(&math.body));
    }

    #[test]
    fn replace_and_insert_operate_on_nested_slots() {
        let mut math = Math::new(Node::row([
            Node::variable('a'),
            Node::Sqrt(Box::new(Sqrt::square(Node::variable('x')))),
        ]));
        let radicand = Path::root().child(1).child(0);
        let replaced = math
            .node_at_mut(&radicand)
            .map(|node| std::mem::replace(node, Node::variable('y')));
        assert_eq!(replaced, Some(Node::variable('x')));
        assert_eq!(math.node_at(&radicand), Some(&Node::variable('y')));

        let row = Path::root();
        assert!(math.body.insert_at(&row, Node::variable('z')));
        assert_eq!(math.body.slot_count(), 3);
        assert_eq!(
            math.node_at(&Path::root().child(2)),
            Some(&Node::variable('z'))
        );
        assert_eq!(
            math.body.remove_at(&Path::root().child(2)),
            Some(Node::variable('z'))
        );
        assert_eq!(math.body.slot_count(), 2);
    }

    #[test]
    fn matrix_cells_are_addressed_row_major() {
        let mut matrix = Matrix::empty(2, 3);
        matrix
            .cell_at_mut(4)
            .map(|cell| *cell = Node::variable('q'));
        let node = Node::Matrix(Box::new(matrix));
        assert_eq!(node.slot_count(), 6);
        assert_eq!(node.slot(4), Some(&Node::variable('q')));
        assert_eq!(node.slot(1), Some(&Node::empty()));
    }

    #[test]
    fn walk_visits_every_reachable_node() {
        let math = Math::new(Node::row([
            scripts(Node::variable('a'), Node::variable('i'), Node::empty()),
            Node::Sqrt(Box::new(Sqrt::square(Node::variable('x')))),
        ]));
        let mut names = Vec::new();
        math.body.walk(&mut |node| {
            if let Node::Char(Char { ch, .. }) = node {
                names.push(*ch);
            }
        });
        assert_eq!(names, vec!['a', 'i', 'x']);
    }

    #[test]
    fn normalisation_flattens_nested_rows_only() {
        let mut node = Node::row([
            Node::variable('a'),
            Node::row([Node::variable('b'), Node::variable('c')]),
        ]);
        node.normalize();
        assert_eq!(
            node,
            Node::row([
                Node::variable('a'),
                Node::variable('b'),
                Node::variable('c')
            ])
        );

        let mut node = Node::row([Node::row([Node::variable('a')])]);
        node.normalize();
        assert_eq!(node, Node::row([Node::variable('a')]));

        let mut fraction = Node::Frac(Box::new(Frac::new(
            Node::row([Node::row([Node::variable('1')])]),
            Node::variable('2'),
        )));
        fraction.normalize();
        assert_eq!(fraction.slot(0), Some(&Node::row([Node::variable('1')])));
    }

    #[test]
    fn space_widths_follow_the_em_grid() {
        assert_eq!(Space::Thin.em(), 3.0 / 18.0);
        assert_eq!(Space::Quad.em(), 1.0);
        assert_eq!(Space::NegativeThick.em(), -5.0 / 18.0);
    }

    #[test]
    fn delimiters_expose_matching_pairs() {
        assert_eq!(Delim::Paren.open(), Some('('));
        assert_eq!(Delim::Paren.close(), Some(')'));
        assert_eq!(Delim::None.open(), None);
        assert_eq!(Delim::Brace.close(), Some('}'));
    }

    #[test]
    fn style_nesting_saturates() {
        assert_eq!(MathStyle::Full.smaller(), MathStyle::Script);
        assert_eq!(MathStyle::Script.smaller(), MathStyle::ScriptScript);
        assert_eq!(MathStyle::ScriptScript.smaller(), MathStyle::ScriptScript);
    }
}
