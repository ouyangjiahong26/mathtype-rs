//! Model layer for the equation editor: the tree that parsers build, the editor mutates and the
//! renderers draw.
//!
//! ```
//! use mt_core::{Math, Node, Path};
//!
//! let mut equation = Math::new(Node::row([Node::variable('a')]));
//! equation.body.insert_at(&Path::root(), Node::variable('b'));
//! assert_eq!(equation.body.slot_count(), 2);
//! ```
#![forbid(unsafe_code)]

pub mod ast;

pub use ast::{
    Accent, AccentKind, BigOp, Char, Class, Delim, Fence, Font, Frac, FracKind, Limits, Math,
    MathStyle, Matrix, MatrixKind, Node, Path, Row, Scripts, Side, Space, Sqrt, Styled, TextRun,
};
