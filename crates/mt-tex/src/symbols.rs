//! Macro tables shared by the parser and the serialiser.
//!
//! Each entry maps a control-sequence name (without the leading backslash) to the character it
//! stands for and the atom class the character carries. Greek letters and symbols that are not
//! ascii letters are set in italics, see [`char_font`].

use mt_core::{Class, Font};

/// Simple control sequences that expand to a single character.
///
/// Names are stored without the leading backslash, e.g. `("alpha", 'α', Class::Ord)`. When two
/// names expand to the same character the first one is used when serialising.
pub(crate) const SYMBOLS: &[(&str, char, Class)] = &[
    // lowercase Greek
    ("alpha", 'α', Class::Ord),
    ("beta", 'β', Class::Ord),
    ("gamma", 'γ', Class::Ord),
    ("delta", 'δ', Class::Ord),
    ("epsilon", 'ε', Class::Ord),
    ("varepsilon", 'ϵ', Class::Ord),
    ("zeta", 'ζ', Class::Ord),
    ("eta", 'η', Class::Ord),
    ("theta", 'θ', Class::Ord),
    ("vartheta", 'ϑ', Class::Ord),
    ("iota", 'ι', Class::Ord),
    ("kappa", 'κ', Class::Ord),
    ("varkappa", 'ϰ', Class::Ord),
    ("lambda", 'λ', Class::Ord),
    ("mu", 'μ', Class::Ord),
    ("nu", 'ν', Class::Ord),
    ("xi", 'ξ', Class::Ord),
    ("omicron", 'ο', Class::Ord),
    ("pi", 'π', Class::Ord),
    ("varpi", 'ϖ', Class::Ord),
    ("rho", 'ρ', Class::Ord),
    ("varrho", 'ϱ', Class::Ord),
    ("sigma", 'σ', Class::Ord),
    ("varsigma", 'ς', Class::Ord),
    ("tau", 'τ', Class::Ord),
    ("upsilon", 'υ', Class::Ord),
    ("phi", 'φ', Class::Ord),
    ("varphi", 'ϕ', Class::Ord),
    ("chi", 'χ', Class::Ord),
    ("psi", 'ψ', Class::Ord),
    ("omega", 'ω', Class::Ord),
    // uppercase Greek
    ("Gamma", 'Γ', Class::Ord),
    ("Delta", 'Δ', Class::Ord),
    ("Theta", 'Θ', Class::Ord),
    ("Lambda", 'Λ', Class::Ord),
    ("Xi", 'Ξ', Class::Ord),
    ("Pi", 'Π', Class::Ord),
    ("Sigma", 'Σ', Class::Ord),
    ("Upsilon", 'Υ', Class::Ord),
    ("Phi", 'Φ', Class::Ord),
    ("Psi", 'Ψ', Class::Ord),
    ("Omega", 'Ω', Class::Ord),
    // ordinary symbols
    ("infty", '∞', Class::Ord),
    ("partial", '∂', Class::Ord),
    ("nabla", '∇', Class::Ord),
    ("forall", '∀', Class::Ord),
    ("exists", '∃', Class::Ord),
    ("nexists", '∄', Class::Ord),
    ("emptyset", '∅', Class::Ord),
    ("varnothing", '∅', Class::Ord),
    ("angle", '∠', Class::Ord),
    ("aleph", 'ℵ', Class::Ord),
    ("hbar", 'ℏ', Class::Ord),
    ("ell", 'ℓ', Class::Ord),
    ("Re", 'ℜ', Class::Ord),
    ("Im", 'ℑ', Class::Ord),
    ("prime", '′', Class::Ord),
    ("degree", '°', Class::Ord),
    ("ldots", '…', Class::Ord),
    ("cdots", '⋯', Class::Ord),
    ("vdots", '⋮', Class::Ord),
    ("ddots", '⋱', Class::Ord),
    // binary operators
    ("pm", '±', Class::Bin),
    ("mp", '∓', Class::Bin),
    ("times", '×', Class::Bin),
    ("div", '÷', Class::Bin),
    ("cdot", '⋅', Class::Bin),
    ("ast", '∗', Class::Bin),
    ("star", '⋆', Class::Bin),
    ("circ", '∘', Class::Bin),
    ("bullet", '∙', Class::Bin),
    ("oplus", '⊕', Class::Bin),
    ("otimes", '⊗', Class::Bin),
    ("odot", '⊙', Class::Bin),
    ("setminus", '∖', Class::Bin),
    ("cup", '∪', Class::Bin),
    ("cap", '∩', Class::Bin),
    ("vee", '∨', Class::Bin),
    ("wedge", '∧', Class::Bin),
    // relations
    ("leq", '≤', Class::Rel),
    ("le", '≤', Class::Rel),
    ("geq", '≥', Class::Rel),
    ("ge", '≥', Class::Rel),
    ("neq", '≠', Class::Rel),
    ("ne", '≠', Class::Rel),
    ("approx", '≈', Class::Rel),
    ("equiv", '≡', Class::Rel),
    ("sim", '∼', Class::Rel),
    ("simeq", '≃', Class::Rel),
    ("cong", '≅', Class::Rel),
    ("propto", '∝', Class::Rel),
    ("ll", '≪', Class::Rel),
    ("gg", '≫', Class::Rel),
    ("subset", '⊂', Class::Rel),
    ("supset", '⊃', Class::Rel),
    ("subseteq", '⊆', Class::Rel),
    ("supseteq", '⊇', Class::Rel),
    ("in", '∈', Class::Rel),
    ("notin", '∉', Class::Rel),
    ("ni", '∋', Class::Rel),
    ("perp", '⊥', Class::Rel),
    ("parallel", '∥', Class::Rel),
    ("mid", '∣', Class::Rel),
    ("nmid", '∤', Class::Rel),
    ("to", '→', Class::Rel),
    ("rightarrow", '→', Class::Rel),
    ("leftarrow", '←', Class::Rel),
    ("leftrightarrow", '↔', Class::Rel),
    ("Rightarrow", '⇒', Class::Rel),
    ("Leftarrow", '⇐', Class::Rel),
    ("Leftrightarrow", '⇔', Class::Rel),
    ("mapsto", '↦', Class::Rel),
    ("uparrow", '↑', Class::Rel),
    ("downarrow", '↓', Class::Rel),
    ("implies", '⟹', Class::Rel),
    ("iff", '⟺', Class::Rel),
    // delimiter characters that are written as macros
    ("{", '{', Class::Open),
    ("}", '}', Class::Close),
    ("lbrace", '{', Class::Open),
    ("rbrace", '}', Class::Close),
    ("|", '‖', Class::Ord),
    ("Vert", '‖', Class::Ord),
    ("langle", '⟨', Class::Open),
    ("rangle", '⟩', Class::Close),
    ("lfloor", '⌊', Class::Open),
    ("rfloor", '⌋', Class::Close),
    ("lceil", '⌈', Class::Open),
    ("rceil", '⌉', Class::Close),
];

/// Look up a control-sequence name.
pub(crate) fn lookup(name: &str) -> Option<(char, Class)> {
    SYMBOLS.iter().find(|(n, _, _)| *n == name).map(|(_, ch, class)| (*ch, *class))
}

/// The canonical macro name for a character, if the character has one.
pub(crate) fn macro_for(ch: char) -> Option<&'static str> {
    SYMBOLS.iter().find(|(_, c, _)| *c == ch).map(|(name, _, _)| *name)
}

/// The class a character carries when it appears in the table.
pub(crate) fn lookup_by_char(ch: char) -> Option<(char, Class)> {
    SYMBOLS.iter().find(|(_, c, _)| *c == ch).map(|(_, ch, class)| (*ch, *class))
}

/// Large operators, with a flag saying whether they take limits above and below by default.
pub(crate) const BIG_OPS: &[(&str, char, bool)] = &[
    ("sum", '∑', true),
    ("prod", '∏', true),
    ("coprod", '∐', true),
    ("bigcup", '⋃', true),
    ("bigcap", '⋂', true),
    ("bigvee", '⋁', true),
    ("bigwedge", '⋀', true),
    ("biguplus", '⨄', true),
    ("int", '∫', false),
    ("iint", '∬', false),
    ("iiint", '∭', false),
    ("oint", '∮', false),
];

/// Named operators written as text, with their default limit placement.
pub(crate) const NAMED_OPS: &[(&str, bool)] = &[
    ("lim", true),
    ("max", true),
    ("min", true),
    ("sup", true),
    ("inf", true),
    ("det", true),
    ("gcd", true),
    ("limsup", true),
    ("liminf", true),
    ("log", false),
    ("ln", false),
    ("exp", false),
    ("sin", false),
    ("cos", false),
    ("tan", false),
    ("cot", false),
    ("sec", false),
    ("csc", false),
    ("arcsin", false),
    ("arccos", false),
    ("arctan", false),
    ("sinh", false),
    ("cosh", false),
    ("tanh", false),
];

/// Character and default limit placement for a large operator macro.
pub(crate) fn big_op(name: &str) -> Option<(char, bool)> {
    BIG_OPS.iter().find(|(n, _, _)| *n == name).map(|(_, ch, limits)| (*ch, *limits))
}

/// Macro name for a large-operator character.
pub(crate) fn big_op_macro(ch: char) -> Option<&'static str> {
    BIG_OPS.iter().find(|(_, c, _)| *c == ch).map(|(name, _, _)| *name)
}

/// Default limit placement for a large-operator character.
pub(crate) fn big_op_limits(ch: char) -> Option<bool> {
    BIG_OPS.iter().find(|(_, c, _)| *c == ch).map(|(_, _, limits)| *limits)
}

/// Whether a named operator is known, and its default limit placement.
pub(crate) fn named_op(name: &str) -> Option<bool> {
    NAMED_OPS.iter().find(|(n, _)| *n == name).map(|(_, limits)| *limits)
}

/// True when the character is drawn in italics by default: ascii letters and lowercase Greek.
pub(crate) fn char_font(ch: char) -> Font {
    if ch.is_ascii_alphabetic() || is_lower_greek(ch) {
        Font::Italic
    } else {
        Font::Upright
    }
}

fn is_lower_greek(ch: char) -> bool {
    matches!(ch as u32, 0x3B1..=0x3C9 | 0x3D1 | 0x3D5 | 0x3D6 | 0x3D7 | 0x3F0 | 0x3F1 | 0x3F5)
}
