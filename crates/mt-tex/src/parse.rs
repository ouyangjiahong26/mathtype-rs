//! Recursive-descent parser turning LaTeX math into the frozen [`mt_core::Math`] tree.
//!
//! The parser owns no state beyond the cursor into the input, so every error carries the byte
//! offset of the offending token.

use mt_core::{
    Accent, AccentKind, BigOp, Class, Delim, Fence, Font, Frac, FracKind, Limits, Math, MathStyle,
    Matrix, MatrixKind, Node, Scripts, Side, Space, Sqrt, Styled, TextRun,
};

use crate::symbols;

/// Error produced when the input is not valid LaTeX math.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// Human readable description of the problem.
    pub message: String,
    /// Byte offset into the original input where the problem was detected.
    pub offset: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (at byte {})", self.message, self.offset)
    }
}

impl std::error::Error for ParseError {}

/// Parse a LaTeX math expression.
///
/// Accepts bare math as well as `$...$`, `$$...$$`, `\(...\)` and `\[...\]` wrappers, which are
/// stripped. The returned equation is always at [`MathStyle::Full`].
pub fn parse(input: &str) -> Result<Math, ParseError> {
    let (body, base) = strip_wrapper(input);
    let mut parser = Parser::new(body, base);
    let row = parser.parse_row(Mode::Normal, false)?;
    parser.skip_spaces();
    if let Some(ch) = parser.peek() {
        return Err(parser.err(&format!("unexpected `{ch}` after the end of the equation")));
    }
    Ok(Math { body: Node::Row(row), style: MathStyle::Full })
}

/// Remove one layer of math delimiters and report the byte offset of the remaining text.
fn strip_wrapper(input: &str) -> (&str, usize) {
    let lead = input.len() - input.trim_start().len();
    let trail = input.len() - input.trim_end().len();
    let trimmed = &input[lead..input.len() - trail];
    let inner = |skip: usize| (&trimmed[skip..trimmed.len() - skip], lead + skip);
    if trimmed.len() >= 4 && trimmed.starts_with("$$") && trimmed.ends_with("$$") {
        inner(2)
    } else if trimmed.len() >= 2 && trimmed.starts_with("\\[") && trimmed.ends_with("\\]") {
        inner(2)
    } else if trimmed.len() >= 2 && trimmed.starts_with("\\(") && trimmed.ends_with("\\)") {
        inner(2)
    } else if trimmed.len() >= 2 && trimmed.starts_with('$') && trimmed.ends_with('$') {
        inner(1)
    } else {
        (input, 0)
    }
}

/// Whether `&` marks an alignment point rather than ending a cell.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// `&` terminates a cell.
    Normal,
    /// `&` inserts a [`Node::Align`] into the row.
    Aligned,
}

struct Parser {
    src: Vec<(usize, char)>,
    pos: usize,
    base: usize,
    input_len: usize,
}

impl Parser {
    fn new(input: &str, base: usize) -> Self {
        Self { src: input.char_indices().collect(), pos: 0, base, input_len: input.len() }
    }

    fn peek(&self) -> Option<char> {
        self.src.get(self.pos).map(|&(_, ch)| ch)
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn offset(&self) -> usize {
        self.src.get(self.pos).map(|&(off, _)| off).unwrap_or(self.input_len) + self.base
    }

    fn err(&self, message: &str) -> ParseError {
        ParseError { message: message.to_owned(), offset: self.offset() }
    }

    /// Skip whitespace. Math mode ignores spaces, tabs and newlines.
    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    /// Name of the control sequence at the cursor, without consuming it.
    fn peek_control_name(&self) -> Option<String> {
        if self.peek() != Some('\\') {
            return None;
        }
        let mut index = self.pos + 1;
        let mut name = String::new();
        while let Some(&(_, ch)) = self.src.get(index) {
            if ch.is_ascii_alphabetic() {
                name.push(ch);
                index += 1;
            } else {
                break;
            }
        }
        if name.is_empty() {
            self.src.get(index).map(|&(_, ch)| ch.to_string())
        } else {
            Some(name)
        }
    }

    fn control_is(&self, name: &str) -> bool {
        self.peek_control_name().is_some_and(|n| n == name)
    }

    /// Consume a control sequence and return its name plus the byte offset of the backslash.
    fn read_control(&mut self) -> Result<(String, usize), ParseError> {
        let start = self.offset();
        debug_assert_eq!(self.peek(), Some('\\'));
        self.bump();
        let mut name = String::new();
        while matches!(self.peek(), Some(c) if c.is_ascii_alphabetic()) {
            name.push(self.bump().unwrap());
        }
        if name.is_empty() {
            match self.bump() {
                Some(ch) => Ok((ch.to_string(), start)),
                None => Err(ParseError {
                    message: "trailing `\\` without a control sequence".to_owned(),
                    offset: start,
                }),
            }
        } else {
            Ok((name, start))
        }
    }

    // ---------------------------------------------------------------- rows

    /// Parse a sequence of atoms until a terminator owned by the caller is reached.
    fn parse_row(&mut self, mode: Mode, bracket: bool) -> Result<Vec<Node>, ParseError> {
        let mut out: Vec<Node> = Vec::new();
        let mut over: Option<Vec<Node>> = None;
        let mut pending_style: Option<MathStyle> = None;
        loop {
            self.skip_spaces();
            match self.peek() {
                None => break,
                Some('}') => break,
                Some(']') if bracket => break,
                Some('&') => {
                    if mode == Mode::Aligned {
                        self.bump();
                        out.push(Node::Align);
                        continue;
                    }
                    break;
                }
                Some('^') | Some('_') => {
                    return Err(self.err("script without a base"));
                }
                Some('\\') => {
                    let name = self.peek_control_name();
                    match name.as_deref() {
                        Some("over") => {
                            self.read_control()?;
                            if over.is_some() {
                                return Err(self.err("more than one `\\over` in a group"));
                            }
                            over = Some(std::mem::take(&mut out));
                        }
                        Some("right") | Some("end") | Some("\\") => break,
                        _ if name.as_deref().and_then(style_control).is_some() => {
                            let (raw, _) = self.read_control()?;
                            pending_style = style_control(&raw);
                        }
                        _ => out.push(self.parse_atom_scripts()?),
                    }
                }
                Some(_) => out.push(self.parse_atom_scripts()?),
            }
        }
        if let Some(num) = over {
            out = vec![Node::Frac(Box::new(Frac {
                num: group_node(num),
                den: group_node(out),
                kind: FracKind::Bar,
            }))];
        }
        if let Some(style) = pending_style {
            let body = group_node(out);
            out = vec![Node::Styled(Box::new(Styled {
                body,
                style: Some(style),
                font: None,
                class: None,
            }))];
        }
        Ok(out)
    }

    /// Parse one atom and attach any following scripts.
    fn parse_atom_scripts(&mut self) -> Result<Node, ParseError> {
        let base = self.parse_atom()?;
        self.attach_scripts(base)
    }

    fn parse_atom(&mut self) -> Result<Node, ParseError> {
        self.skip_spaces();
        match self.peek() {
            None => Err(self.err("unexpected end of input")),
            Some('{') => self.parse_group(),
            Some('}') => Err(self.err("unmatched `}`")),
            Some('&') => Err(self.err("alignment tab outside an environment")),
            Some('^') | Some('_') => Err(self.err("script without a base")),
            Some('$') => Err(self.err("unexpected `$`")),
            Some('\\') => self.parse_control(),
            Some(ch) => {
                self.bump();
                Ok(char_node(ch))
            }
        }
    }

    /// Parse `{ ... }`, honouring `\over` inside the group.
    fn parse_group(&mut self) -> Result<Node, ParseError> {
        debug_assert_eq!(self.peek(), Some('{'));
        self.bump();
        let row = self.parse_row(Mode::Normal, false)?;
        self.skip_spaces();
        if self.peek() != Some('}') {
            return Err(self.err("unclosed `{`"));
        }
        self.bump();
        Ok(group_node(row))
    }

    /// Parse a required argument: a braced group, or a single atom.
    ///
    /// A single atom is returned bare, matching the canonical slot form.
    fn parse_arg(&mut self) -> Result<Node, ParseError> {
        self.skip_spaces();
        if self.peek() == Some('{') {
            self.parse_group()
        } else {
            self.parse_atom()
        }
    }

    /// Attach `^`/`_` scripts (and `\limits`/`\nolimits`) to a base.
    fn attach_scripts(&mut self, base: Node) -> Result<Node, ParseError> {
        let mut sub = None;
        let mut sup = None;
        let mut limits_flag = None;
        loop {
            self.skip_spaces();
            match self.peek() {
                Some('^') => {
                    self.bump();
                    if sup.is_some() {
                        return Err(self.err("double superscript"));
                    }
                    sup = Some(self.parse_arg()?);
                }
                Some('_') => {
                    self.bump();
                    if sub.is_some() {
                        return Err(self.err("double subscript"));
                    }
                    sub = Some(self.parse_arg()?);
                }
                Some('\\') => {
                    if self.control_is("limits") {
                        self.read_control()?;
                        limits_flag = Some(true);
                    } else if self.control_is("nolimits") {
                        self.read_control()?;
                        limits_flag = Some(false);
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        if let Node::Big(mut big) = base {
            if let Some(flag) = limits_flag {
                big.limits = flag;
            }
            if sub.is_some() {
                big.sub = sub;
            }
            if sup.is_some() {
                big.sup = sup;
            }
            return Ok(Node::Big(big));
        }
        if sub.is_none() && sup.is_none() {
            return Ok(base);
        }
        Ok(Node::Scripts(Box::new(Scripts { base, sub, sup })))
    }

    // ------------------------------------------------------------ controls

    fn parse_control(&mut self) -> Result<Node, ParseError> {
        let (name, start) = self.read_control()?;
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let num = self.parse_arg()?;
                let den = self.parse_arg()?;
                let frac = Node::Frac(Box::new(Frac { num, den, kind: FracKind::Bar }));
                Ok(match name.as_str() {
                    "dfrac" => styled_style(MathStyle::Full, frac),
                    "tfrac" => styled_style(MathStyle::Text, frac),
                    _ => frac,
                })
            }
            "binom" => {
                let num = self.parse_arg()?;
                let den = self.parse_arg()?;
                let frac = Node::Frac(Box::new(Frac { num, den, kind: FracKind::NoBar }));
                Ok(Node::Fence(Box::new(Fence {
                    open: Delim::Paren,
                    close: Delim::Paren,
                    body: frac,
                })))
            }
            "genfrac" => self.parse_genfrac(),
            "sqrt" => {
                self.skip_spaces();
                let index = if self.peek() == Some('[') { Some(self.parse_bracket()?) } else { None };
                let radicand = self.parse_arg()?;
                Ok(Node::Sqrt(Box::new(Sqrt { index, radicand })))
            }
            "left" => self.parse_left(),
            "begin" => self.parse_environment(start),
            "substack" => self.parse_substack(),
            "operatorname" => {
                let limits = if self.peek() == Some('*') {
                    self.bump();
                    true
                } else {
                    false
                };
                let text = self.read_brace_string()?;
                let body = Node::Text(Box::new(TextRun { text, font: Font::Upright }));
                if limits {
                    Ok(Node::Big(Box::new(BigOp { base: body, sub: None, sup: None, limits: true })))
                } else {
                    Ok(Node::Styled(Box::new(Styled {
                        body,
                        style: None,
                        font: None,
                        class: Some(Class::Op),
                    })))
                }
            }
            "text" | "textrm" | "textnormal" | "textbf" | "textit" | "texttt" | "textsf" => {
                let font = text_font(&name);
                let text = self.read_brace_string()?;
                Ok(Node::Text(Box::new(TextRun { text, font })))
            }
            "mathbb" | "mathcal" | "mathfrak" | "mathbf" | "mathit" | "mathrm" | "mathsf"
            | "mathtt" | "boldsymbol" | "bm" | "mathnormal" => {
                let font = font_macro(&name);
                let body = self.parse_arg()?;
                Ok(styled_font(font, body))
            }
            "hspace" => self.parse_hspace(),
            "quad" => Ok(Node::Space(Space::Quad)),
            "qquad" => Ok(Node::Space(Space::DoubleQuad)),
            "," | ":" | ";" | "!" | " " => Ok(Node::Space(space_control(&name))),
            "overset" | "stackrel" => {
                let over = self.parse_arg()?;
                let base = self.parse_arg()?;
                Ok(Node::Limits(Box::new(Limits { base, under: None, over: Some(over), align: Side::Center })))
            }
            "underset" => {
                let under = self.parse_arg()?;
                let base = self.parse_arg()?;
                Ok(Node::Limits(Box::new(Limits { base, under: Some(under), over: None, align: Side::Center })))
            }
            "overleftrightarrow" => {
                let base = self.parse_arg()?;
                Ok(Node::Limits(Box::new(Limits {
                    base,
                    under: None,
                    over: Some(Node::symbol('↔', Class::Rel)),
                    align: Side::Center,
                })))
            }
            "not" => {
                let base = self.parse_arg()?;
                Ok(Node::Accent(Box::new(Accent { kind: AccentKind::Strike, base, under: false })))
            }
            _ => {
                if let Some((kind, under)) = accent_macro(&name) {
                    let base = self.parse_arg()?;
                    return Ok(Node::Accent(Box::new(Accent { kind, base, under })));
                }
                if let Some(class) = big_size_class(&name) {
                    let delim = self.parse_delim()?;
                    let ch = match class {
                        Class::Close => delim.close(),
                        _ => delim.open(),
                    };
                    let ch = ch.ok_or_else(|| ParseError {
                        message: format!("`\\{name}` needs a delimiter"),
                        offset: start,
                    })?;
                    return Ok(Node::Char(mt_core::Char { ch, class, font: Font::Upright }));
                }
                if let Some((ch, limits)) = symbols::big_op(&name) {
                    let base = char_node_with(ch, Class::Op, Font::Upright);
                    return Ok(Node::Big(Box::new(BigOp { base, sub: None, sup: None, limits })));
                }
                if let Some(limits) = symbols::named_op(&name) {
                    let body = Node::Text(Box::new(TextRun { text: name.clone(), font: Font::Upright }));
                    return Ok(Node::Big(Box::new(BigOp { base: body, sub: None, sup: None, limits })));
                }
                if let Some((ch, class)) = symbols::lookup(&name) {
                    return Ok(char_node_with(ch, class, symbols::char_font(ch)));
                }
                if let Some(style) = style_control(&name) {
                    return Ok(styled_style(style, Node::empty()));
                }
                Err(ParseError {
                    message: format!("unknown control sequence `\\{name}`"),
                    offset: start,
                })
            }
        }
    }

    /// Parse `\genfrac{open}{close}{thickness}{style}{num}{den}`.
    fn parse_genfrac(&mut self) -> Result<Node, ParseError> {
        let open = self.read_brace_string()?;
        let close = self.read_brace_string()?;
        let thickness = self.read_brace_string()?;
        let style = self.read_brace_string()?;
        let num = self.parse_arg()?;
        let den = self.parse_arg()?;
        let kind = if thickness.trim().is_empty() { FracKind::NoBar } else { FracKind::Bar };
        let mut node = Node::Frac(Box::new(Frac { num, den, kind }));
        let open_delim = delim_from_str(&open);
        let close_delim = delim_from_str(&close);
        if open_delim != Delim::None || close_delim != Delim::None {
            node = Node::Fence(Box::new(Fence { open: open_delim, close: close_delim, body: node }));
        }
        if let Some(style) = genfrac_style(&style) {
            node = styled_style(style, node);
        }
        Ok(node)
    }

    fn parse_left(&mut self) -> Result<Node, ParseError> {
        let open = self.parse_delim()?;
        let body = self.parse_row(Mode::Normal, false)?;
        self.skip_spaces();
        if !self.control_is("right") {
            return Err(self.err("`\\left` without a matching `\\right`"));
        }
        self.read_control()?;
        let close = self.parse_delim()?;
        Ok(Node::Fence(Box::new(Fence { open, close, body: group_node(body) })))
    }

    fn parse_substack(&mut self) -> Result<Node, ParseError> {
        self.skip_spaces();
        if self.peek() != Some('{') {
            return Err(self.err("`\\substack` needs a braced argument"));
        }
        self.bump();
        let mut rows = Vec::new();
        loop {
            let row = self.parse_row(Mode::Normal, false)?;
            rows.push(vec![group_node(row)]);
            self.skip_spaces();
            if self.control_is("\\") {
                self.read_control()?;
                continue;
            }
            if self.peek() == Some('}') {
                self.bump();
                break;
            }
            return Err(self.err("expected `\\\\` or `}` in `\\substack`"));
        }
        Ok(Node::Matrix(Box::new(Matrix {
            rows,
            delims: None,
            kind: MatrixKind::Pile,
            align: Side::Center,
            columns: Vec::new(),
            rules: false,
        })))
    }

    fn parse_environment(&mut self, start: usize) -> Result<Node, ParseError> {
        let name = self.read_brace_string()?;
        let (kind, delims, align, columns, rules) = match name.as_str() {
            "matrix" | "smallmatrix" => (MatrixKind::Matrix, None, Side::Center, Vec::new(), false),
            "pmatrix" => (MatrixKind::Matrix, Some((Delim::Paren, Delim::Paren)), Side::Center, Vec::new(), false),
            "bmatrix" => (MatrixKind::Matrix, Some((Delim::Bracket, Delim::Bracket)), Side::Center, Vec::new(), false),
            "Bmatrix" => (MatrixKind::Matrix, Some((Delim::Brace, Delim::Brace)), Side::Center, Vec::new(), false),
            "vmatrix" => (MatrixKind::Matrix, Some((Delim::Abs, Delim::Abs)), Side::Center, Vec::new(), false),
            "Vmatrix" => (MatrixKind::Matrix, Some((Delim::Norm, Delim::Norm)), Side::Center, Vec::new(), false),
            "cases" => (MatrixKind::Cases, Some((Delim::Brace, Delim::None)), Side::Left, vec![Side::Left, Side::Left], false),
            "array" => {
                let spec = self.read_brace_string()?;
                (MatrixKind::Array, None, Side::Center, parse_column_spec(&spec, start)?, true)
            }
            "aligned" => (MatrixKind::Aligned, None, Side::Center, Vec::new(), false),
            "gathered" => (MatrixKind::Pile, None, Side::Center, Vec::new(), false),
            _ => {
                return Err(ParseError {
                    message: format!("unknown environment `{name}`"),
                    offset: start,
                });
            }
        };
        let rows = self.parse_environment_body(&name, kind)?;
        Ok(Node::Matrix(Box::new(Matrix { rows, delims, kind, align, columns, rules })))
    }

    fn parse_environment_body(&mut self, name: &str, kind: MatrixKind) -> Result<Vec<Vec<Node>>, ParseError> {
        let mode = if kind == MatrixKind::Aligned { Mode::Aligned } else { Mode::Normal };
        let mut rows = Vec::new();
        loop {
            self.skip_spaces();
            if self.control_is("end") {
                self.read_control()?;
                let end_name = self.read_brace_string()?;
                if end_name != name {
                    return Err(self.err(&format!(
                        "`\\end{{{end_name}}}` does not match `\\begin{{{name}}}`"
                    )));
                }
                break;
            }
            if self.peek().is_none() {
                return Err(self.err(&format!("unclosed environment `{name}`")));
            }
            let mut cells = Vec::new();
            loop {
                let row = self.parse_row(mode, false)?;
                cells.push(group_node(row));
                self.skip_spaces();
                if self.peek() == Some('&') {
                    self.bump();
                    continue;
                }
                break;
            }
            rows.push(cells);
            self.skip_spaces();
            if self.control_is("\\") {
                self.read_control()?;
                continue;
            }
            if self.control_is("end") {
                continue;
            }
            if self.peek().is_none() {
                return Err(self.err(&format!("unclosed environment `{name}`")));
            }
            return Err(self.err("expected `\\\\` or `\\end` in environment"));
        }
        Ok(rows)
    }

    fn parse_hspace(&mut self) -> Result<Node, ParseError> {
        let spec = self.read_brace_string()?;
        parse_length(&spec)
            .map(Node::Space)
            .map_err(|message| ParseError { message, offset: self.offset() })
    }

    // ---------------------------------------------------------- primitives

    fn parse_bracket(&mut self) -> Result<Node, ParseError> {
        debug_assert_eq!(self.peek(), Some('['));
        self.bump();
        let row = self.parse_row(Mode::Normal, true)?;
        self.skip_spaces();
        if self.peek() != Some(']') {
            return Err(self.err("unclosed `[`"));
        }
        self.bump();
        Ok(group_node(row))
    }

    /// Parse one delimiter, either a plain character or a control sequence.
    fn parse_delim(&mut self) -> Result<Delim, ParseError> {
        self.skip_spaces();
        match self.peek() {
            None => Err(self.err("expected a delimiter")),
            Some('\\') => {
                let (name, start) = self.read_control()?;
                match name.as_str() {
                    "{" | "}" | "lbrace" | "rbrace" => Ok(Delim::Brace),
                    "|" | "Vert" | "lVert" | "rVert" => Ok(Delim::DoubleBar),
                    "langle" | "rangle" => Ok(Delim::Angle),
                    "lfloor" | "rfloor" => Ok(Delim::Floor),
                    "lceil" | "rceil" => Ok(Delim::Ceil),
                    "(" | ")" => Ok(Delim::Paren),
                    "[" | "]" => Ok(Delim::Bracket),
                    "uparrow" => Ok(Delim::Custom('↑')),
                    "downarrow" => Ok(Delim::Custom('↓')),
                    "." => Ok(Delim::None),
                    other if other.chars().count() == 1 => Ok(Delim::Custom(other.chars().next().unwrap())),
                    _ => Err(ParseError { message: format!("`\\{name}` is not a delimiter"), offset: start }),
                }
            }
            Some('(') | Some(')') => {
                self.bump();
                Ok(Delim::Paren)
            }
            Some('[') | Some(']') => {
                self.bump();
                Ok(Delim::Bracket)
            }
            Some('{') | Some('}') => {
                self.bump();
                Ok(Delim::Brace)
            }
            Some('|') => {
                self.bump();
                Ok(Delim::Abs)
            }
            Some('.') => {
                self.bump();
                Ok(Delim::None)
            }
            Some(ch) => {
                self.bump();
                Ok(Delim::Custom(ch))
            }
        }
    }

    /// Read a `{...}` group as a raw string, processing text escapes.
    fn read_brace_string(&mut self) -> Result<String, ParseError> {
        self.skip_spaces();
        if self.peek() != Some('{') {
            return Err(self.err("expected `{`"));
        }
        self.bump();
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err(self.err("unclosed `{`")),
                Some('}') => {
                    self.bump();
                    break;
                }
                Some('\\') => {
                    let (name, start) = self.read_control()?;
                    match name.as_str() {
                        "{" => out.push('{'),
                        "}" => out.push('}'),
                        "&" => out.push('&'),
                        "%" => out.push('%'),
                        "$" => out.push('$'),
                        "#" => out.push('#'),
                        "_" => out.push('_'),
                        " " => out.push(' '),
                        "backslash" => out.push('\\'),
                        "quad" => out.push(' '),
                        _ => {
                            return Err(ParseError {
                                message: format!("unsupported `\\{name}` inside a text argument"),
                                offset: start,
                            });
                        }
                    }
                }
                Some(ch) => {
                    self.bump();
                    out.push(ch);
                }
            }
        }
        Ok(out)
    }
}

/// Collapse a parsed row used as a slot: a single item becomes the item itself.
///
/// This matches the canonical form the other codecs use for single-item slots, so that
/// serialising and re-parsing any canonical tree is the identity.
fn group_node(row: Vec<Node>) -> Node {
    if row.len() == 1 {
        row.into_iter().next().unwrap()
    } else {
        Node::Row(row)
    }
}

/// Build a character node from a character, choosing class and font from the tables.
fn char_node(ch: char) -> Node {
    if let Some((mapped, class)) = symbols::lookup_by_char(ch) {
        char_node_with(mapped, class, symbols::char_font(mapped))
    } else {
        char_node_with(ch, char_class(ch), symbols::char_font(ch))
    }
}

fn char_node_with(ch: char, class: Class, font: Font) -> Node {
    Node::Char(mt_core::Char { ch, class, font })
}

/// Atom class for a plain character typed directly in math mode.
fn char_class(ch: char) -> Class {
    match ch {
        '(' | '[' => Class::Open,
        ')' | ']' => Class::Close,
        '+' | '-' | '*' => Class::Bin,
        '=' | '<' | '>' | ':' => Class::Rel,
        ',' | ';' => Class::Punct,
        _ => Class::Ord,
    }
}

fn styled_style(style: MathStyle, body: Node) -> Node {
    Node::Styled(Box::new(Styled { body, style: Some(style), font: None, class: None }))
}

fn styled_font(font: Font, body: Node) -> Node {
    Node::Styled(Box::new(Styled { body, style: None, font: Some(font), class: None }))
}

fn style_control(name: &str) -> Option<MathStyle> {
    Some(match name {
        "displaystyle" => MathStyle::Full,
        "textstyle" => MathStyle::Text,
        "scriptstyle" => MathStyle::Script,
        "scriptscriptstyle" => MathStyle::ScriptScript,
        _ => return None,
    })
}

fn font_macro(name: &str) -> Font {
    match name {
        "mathbb" => Font::DoubleStruck,
        "mathcal" => Font::Script,
        "mathfrak" => Font::Fraktur,
        "mathbf" => Font::Bold,
        "mathit" | "mathnormal" => Font::Italic,
        "mathrm" => Font::Upright,
        "mathsf" => Font::SansSerif,
        "mathtt" => Font::Monospace,
        "boldsymbol" | "bm" => Font::BoldItalic,
        _ => Font::Upright,
    }
}

fn text_font(name: &str) -> Font {
    match name {
        "textbf" => Font::Bold,
        "textit" => Font::Italic,
        "texttt" => Font::Monospace,
        "textsf" => Font::SansSerif,
        _ => Font::Upright,
    }
}

fn accent_macro(name: &str) -> Option<(AccentKind, bool)> {
    Some(match name {
        "hat" => (AccentKind::Hat, false),
        "widehat" => (AccentKind::WideHat, false),
        "bar" => (AccentKind::Bar, false),
        "overline" => (AccentKind::Line, false),
        "underline" => (AccentKind::Line, true),
        "vec" => (AccentKind::Vector, false),
        "dot" => (AccentKind::Dot, false),
        "ddot" => (AccentKind::DoubleDot, false),
        "dddot" => (AccentKind::TripleDot, false),
        "tilde" => (AccentKind::Tilde, false),
        "widetilde" => (AccentKind::WideTilde, false),
        "check" => (AccentKind::Check, false),
        "breve" => (AccentKind::Breve, false),
        "acute" => (AccentKind::Acute, false),
        "grave" => (AccentKind::Grave, false),
        "mathring" => (AccentKind::Circle, false),
        "overbrace" => (AccentKind::OverBrace, false),
        "underbrace" => (AccentKind::UnderBrace, true),
        "overrightarrow" => (AccentKind::ArrowRight, false),
        "overleftarrow" => (AccentKind::ArrowLeft, false),
        _ => return None,
    })
}

fn big_size_class(name: &str) -> Option<Class> {
    Some(match name {
        "big" | "Big" | "bigg" | "Bigg" | "bigm" | "Bigm" | "biggm" | "Biggm" => Class::Ord,
        "bigl" | "Bigl" | "biggl" | "Biggl" => Class::Open,
        "bigr" | "Bigr" | "biggr" | "Biggr" => Class::Close,
        _ => return None,
    })
}

fn space_control(name: &str) -> Space {
    match name {
        "," => Space::Thin,
        ":" => Space::Medium,
        ";" => Space::Thick,
        "!" => Space::NegativeThin,
        _ => Space::Sixteenth,
    }
}

fn delim_from_str(text: &str) -> Delim {
    match text.trim() {
        "" | "." => Delim::None,
        "(" | ")" => Delim::Paren,
        "[" | "]" => Delim::Bracket,
        "{" | "}" | "\\{" | "\\}" => Delim::Brace,
        "|" => Delim::Abs,
        "\\|" | "\\Vert" | "\\lVert" | "\\rVert" => Delim::DoubleBar,
        "\\langle" | "\\rangle" => Delim::Angle,
        "\\lfloor" | "\\rfloor" => Delim::Floor,
        "\\lceil" | "\\rceil" => Delim::Ceil,
        other => Delim::Custom(other.chars().next().unwrap()),
    }
}

fn genfrac_style(spec: &str) -> Option<MathStyle> {
    match spec.trim() {
        "0" => Some(MathStyle::Full),
        "1" => Some(MathStyle::Text),
        "2" => Some(MathStyle::Script),
        "3" => Some(MathStyle::ScriptScript),
        _ => None,
    }
}

fn parse_column_spec(spec: &str, start: usize) -> Result<Vec<Side>, ParseError> {
    let mut columns = Vec::new();
    for ch in spec.chars() {
        match ch {
            'l' => columns.push(Side::Left),
            'c' => columns.push(Side::Center),
            'r' => columns.push(Side::Right),
            '|' | ' ' => {}
            other => {
                return Err(ParseError {
                    message: format!("unsupported column specifier `{other}`"),
                    offset: start,
                });
            }
        }
    }
    Ok(columns)
}

/// Parse a TeX length and round it to the nearest representable [`Space`].
fn parse_length(spec: &str) -> Result<Space, String> {
    let spec = spec.trim();
    let split = spec.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(spec.len());
    let (number, unit) = spec.split_at(split);
    let value: f64 = number.trim().parse().map_err(|_| format!("`{spec}` is not a valid length"))?;
    let em = match unit.trim() {
        "em" => value,
        "mu" => value / 18.0,
        "pt" => value / 10.0,
        "ex" => value / 2.0,
        "" => value,
        other => return Err(format!("unsupported unit `{other}` in length")),
    };
    const SPACES: &[Space] = &[
        Space::Thin,
        Space::Medium,
        Space::Thick,
        Space::Sixteenth,
        Space::Quad,
        Space::DoubleQuad,
        Space::NegativeThin,
        Space::NegativeMedium,
        Space::NegativeThick,
    ];
    Ok(*SPACES
        .iter()
        .min_by(|a, b| {
            let da = (a.em() - em).abs();
            let db = (b.em() - em).abs();
            da.partial_cmp(&db).unwrap()
        })
        .unwrap())
}
