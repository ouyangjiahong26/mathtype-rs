//! Layout engine for the equation editor: turns the tree in [`mt_core`] into positioned boxes and
//! into an SVG preview.
//!
//! ```
//! use mt_core::{Math, Node};
//! use mt_layout::{layout, to_svg, ApproxMetrics, LayoutStyle};
//!
//! let equation = Math::new(Node::row([Node::variable('x'), Node::symbol('+', mt_core::Class::Bin), Node::variable('y')]));
//! let boxes = layout(&equation, &ApproxMetrics, &LayoutStyle::display(10.0));
//! let svg = to_svg(&boxes, &LayoutStyle::display(10.0));
//! assert!(svg.contains("<text"));
//! assert!(boxes.width() > 0.0);
//! ```
//!
//! Coordinates are in points. In a laid-out box `x` grows to the right from the left edge of the
//! equation, and `y` grows downwards with the baseline at zero, matching SVG; `height` is the
//! extent above that baseline and `depth` the extent below it. Glyphs and rules carry the position
//! they must be drawn at, so drawing never needs the metrics again.
//!
//! The layout rules are the classic ones: atom classes drive the spacing between neighbours,
//! template argument slots are laid out one nesting level deeper, and the pieces that have to grow
//! (delimiters, radicals, accents) are fitted to the material they cover. All the numbers the
//! engine uses live in [`constants`].

#![forbid(unsafe_code)]

pub mod constants;
mod boxed;
mod layout;
mod metrics;
mod style;
mod svg;

pub use boxed::Box;
pub use layout::{RowAnchor, layout, layout_with_anchors};
pub use metrics::{ApproxMetrics, FontMetrics};
pub use style::LayoutStyle;
pub use svg::{to_svg, view_box};
