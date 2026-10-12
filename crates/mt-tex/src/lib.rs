//! LaTeX math input and output for the `mt-core` equation model.
//!
//! ```
//! use mt_core::Node;
//! use mt_tex::{parse, to_latex};
//!
//! let math = parse("\\frac{a}{b}").unwrap();
//! assert!(matches!(math.body.slot(0), Some(Node::Frac(_))));
//! assert_eq!(to_latex(&math), "\\frac{a}{b}");
//! ```

mod latex;
mod parse;
mod symbols;
#[cfg(test)]
mod tests;

pub use parse::{ParseError, parse};

use mt_core::{Math, MathStyle, Node};

/// Serialise a whole equation to LaTeX.
///
/// Equations that are not at full size are wrapped in `$...$`; full size equations are emitted
/// bare, matching how [`parse`] strips those wrappers.
pub fn to_latex(math: &Math) -> String {
    let fragment = to_latex_fragment(&math.body);
    if math.style == MathStyle::Full {
        fragment
    } else {
        format!("${fragment}$")
    }
}

/// Serialise a single node to a LaTeX fragment.
pub fn to_latex_fragment(node: &Node) -> String {
    latex::render_to_string(node)
}
