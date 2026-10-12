//! Palette contents: the groups of symbols and templates an author picks from.
//!
//! Groups are plain data so the windowing layer can render them without knowing what a template
//! is, and so tests can walk them.

use crate::Template;
use mt_core::{AccentKind, Class, Delim, Font, MatrixKind, Space};

/// One entry of a palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteItem {
    /// A character inserted with an explicit class and font.
    Symbol { ch: char, class: Class, font: Font },
    /// A template inserted at the caret.
    Template(Template),
    /// Explicit horizontal space inserted at the caret.
    Space(Space),
}

impl PaletteItem {
    /// A symbol drawn from the text face.
    pub const fn symbol(ch: char, class: Class) -> Self {
        PaletteItem::Symbol {
            ch,
            class,
            font: Font::Upright,
        }
    }

    /// A variable drawn in the italic math face.
    pub const fn variable(ch: char) -> Self {
        PaletteItem::Symbol {
            ch,
            class: Class::Ord,
            font: Font::Italic,
        }
    }

    /// A symbol drawn from the Greek/symbol face.
    pub const fn greek(ch: char) -> Self {
        PaletteItem::Symbol {
            ch,
            class: Class::Ord,
            font: Font::Symbol,
        }
    }

    /// A short human-readable description, used as the tooltip.
    pub fn label(self) -> String {
        match self {
            PaletteItem::Symbol { ch, .. } => ch.to_string(),
            PaletteItem::Template(template) => template.name().to_owned(),
            PaletteItem::Space(space) => format!("{} space", space_em(space)),
        }
    }
}

fn space_em(space: Space) -> String {
    let em = space.em();
    let sign = if em < 0.0 { "-" } else { "" };
    format!("{sign}{:.0}/18", em.abs() * 18.0)
}

/// A named group of palette entries laid out in columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub name: &'static str,
    pub columns: &'static [&'static [PaletteItem]],
}

/// Every palette of the editor toolbar, in toolbar order.
pub fn palettes() -> &'static [Palette] {
    &PALETTES
}

static PALETTES: [Palette; 11] = [
    // 1. operators, relations and set theory
    Palette {
        name: "运算符与关系符",
        columns: &[
            &[
                PaletteItem::symbol('\u{00D7}', Class::Bin),
                PaletteItem::symbol('\u{00F7}', Class::Bin),
                PaletteItem::symbol('\u{00B1}', Class::Bin),
                PaletteItem::symbol('\u{2213}', Class::Bin),
                PaletteItem::symbol('\u{22C5}', Class::Bin),
                PaletteItem::symbol('\u{2217}', Class::Bin),
            ],
            &[
                PaletteItem::symbol('\u{2264}', Class::Rel),
                PaletteItem::symbol('\u{2265}', Class::Rel),
                PaletteItem::symbol('\u{2260}', Class::Rel),
                PaletteItem::symbol('\u{2248}', Class::Rel),
                PaletteItem::symbol('\u{2261}', Class::Rel),
                PaletteItem::symbol('\u{223C}', Class::Rel),
            ],
            &[
                PaletteItem::symbol('\u{226A}', Class::Rel),
                PaletteItem::symbol('\u{226B}', Class::Rel),
                PaletteItem::symbol('\u{221D}', Class::Rel),
                PaletteItem::symbol('\u{2245}', Class::Rel),
                PaletteItem::symbol('\u{221E}', Class::Ord),
                PaletteItem::symbol('\u{2205}', Class::Ord),
            ],
            &[
                PaletteItem::symbol('\u{2282}', Class::Rel),
                PaletteItem::symbol('\u{2283}', Class::Rel),
                PaletteItem::symbol('\u{2286}', Class::Rel),
                PaletteItem::symbol('\u{2287}', Class::Rel),
                PaletteItem::symbol('\u{2208}', Class::Rel),
                PaletteItem::symbol('\u{2209}', Class::Rel),
            ],
            &[
                PaletteItem::symbol('\u{2229}', Class::Bin),
                PaletteItem::symbol('\u{222A}', Class::Bin),
                PaletteItem::symbol('\u{2227}', Class::Bin),
                PaletteItem::symbol('\u{2228}', Class::Bin),
                PaletteItem::symbol('\u{2200}', Class::Ord),
                PaletteItem::symbol('\u{2203}', Class::Ord),
            ],
            &[
                PaletteItem::symbol('\u{2234}', Class::Rel),
                PaletteItem::symbol('\u{2235}', Class::Rel),
                PaletteItem::symbol('\u{22A5}', Class::Rel),
                PaletteItem::symbol('\u{2225}', Class::Rel),
                PaletteItem::symbol('\u{2220}', Class::Ord),
                PaletteItem::symbol('\u{2223}', Class::Rel),
            ],
        ],
    },
    // 2. Greek letters
    Palette {
        name: "希腊字母",
        columns: &[
            &[
                PaletteItem::greek('\u{03B1}'),
                PaletteItem::greek('\u{03B2}'),
                PaletteItem::greek('\u{03B3}'),
                PaletteItem::greek('\u{03B4}'),
                PaletteItem::greek('\u{03B5}'),
                PaletteItem::greek('\u{03B6}'),
            ],
            &[
                PaletteItem::greek('\u{03B7}'),
                PaletteItem::greek('\u{03B8}'),
                PaletteItem::greek('\u{03B9}'),
                PaletteItem::greek('\u{03BA}'),
                PaletteItem::greek('\u{03BB}'),
                PaletteItem::greek('\u{03BC}'),
            ],
            &[
                PaletteItem::greek('\u{03BD}'),
                PaletteItem::greek('\u{03BE}'),
                PaletteItem::greek('\u{03C0}'),
                PaletteItem::greek('\u{03C1}'),
                PaletteItem::greek('\u{03C3}'),
                PaletteItem::greek('\u{03C4}'),
            ],
            &[
                PaletteItem::greek('\u{03C5}'),
                PaletteItem::greek('\u{03C6}'),
                PaletteItem::greek('\u{03C7}'),
                PaletteItem::greek('\u{03C8}'),
                PaletteItem::greek('\u{03C9}'),
                PaletteItem::greek('\u{03D1}'),
            ],
            &[
                PaletteItem::greek('\u{0393}'),
                PaletteItem::greek('\u{0394}'),
                PaletteItem::greek('\u{0398}'),
                PaletteItem::greek('\u{039B}'),
                PaletteItem::greek('\u{039E}'),
                PaletteItem::greek('\u{03A0}'),
            ],
            &[
                PaletteItem::greek('\u{03A3}'),
                PaletteItem::greek('\u{03A6}'),
                PaletteItem::greek('\u{03A8}'),
                PaletteItem::greek('\u{03A9}'),
                PaletteItem::symbol('\u{2202}', Class::Ord),
                PaletteItem::symbol('\u{2207}', Class::Ord),
            ],
        ],
    },
    // 3. arrows and miscellaneous symbols
    Palette {
        name: "箭头与其它符号",
        columns: &[
            &[
                PaletteItem::symbol('\u{2192}', Class::Rel),
                PaletteItem::symbol('\u{2190}', Class::Rel),
                PaletteItem::symbol('\u{2194}', Class::Rel),
                PaletteItem::symbol('\u{21D2}', Class::Rel),
                PaletteItem::symbol('\u{21D0}', Class::Rel),
                PaletteItem::symbol('\u{21D4}', Class::Rel),
            ],
            &[
                PaletteItem::symbol('\u{2191}', Class::Rel),
                PaletteItem::symbol('\u{2193}', Class::Rel),
                PaletteItem::symbol('\u{2195}', Class::Rel),
                PaletteItem::symbol('\u{21D1}', Class::Rel),
                PaletteItem::symbol('\u{21D3}', Class::Rel),
                PaletteItem::symbol('\u{21A6}', Class::Rel),
            ],
            &[
                PaletteItem::symbol('\u{2032}', Class::Ord),
                PaletteItem::symbol('\u{2033}', Class::Ord),
                PaletteItem::symbol('\u{210F}', Class::Ord),
                PaletteItem::symbol('\u{2113}', Class::Ord),
                PaletteItem::symbol('\u{211C}', Class::Ord),
                PaletteItem::symbol('\u{2111}', Class::Ord),
            ],
            &[
                PaletteItem::symbol('\u{2135}', Class::Ord),
                PaletteItem::symbol('\u{2218}', Class::Bin),
                PaletteItem::symbol('\u{2295}', Class::Bin),
                PaletteItem::symbol('\u{2297}', Class::Bin),
                PaletteItem::symbol('\u{2299}', Class::Bin),
                PaletteItem::symbol('\u{2216}', Class::Bin),
            ],
            &[
                PaletteItem::symbol('\u{2026}', Class::Ord),
                PaletteItem::symbol('\u{22EF}', Class::Ord),
                PaletteItem::symbol('\u{22EE}', Class::Ord),
                PaletteItem::symbol('\u{22F1}', Class::Ord),
                PaletteItem::symbol('\u{2212}', Class::Bin),
                PaletteItem::symbol('\u{2032}', Class::Ord),
            ],
            &[
                PaletteItem::symbol('\u{00A7}', Class::Ord),
                PaletteItem::symbol('\u{00B6}', Class::Ord),
                PaletteItem::symbol('\u{2020}', Class::Ord),
                PaletteItem::symbol('\u{2021}', Class::Ord),
                PaletteItem::symbol('\u{00AC}', Class::Ord),
                PaletteItem::symbol('\u{2205}', Class::Ord),
            ],
        ],
    },
    // 4. fractions and radicals
    Palette {
        name: "分式与根式",
        columns: &[
            &[
                PaletteItem::Template(Template::Fraction),
                PaletteItem::Template(Template::SlashFraction),
                PaletteItem::Template(Template::Binomial),
            ],
            &[
                PaletteItem::Template(Template::SquareRoot),
                PaletteItem::Template(Template::NthRoot),
            ],
        ],
    },
    // 5. scripts and limits
    Palette {
        name: "上下标与极限",
        columns: &[
            &[
                PaletteItem::Template(Template::SubSuperscript),
                PaletteItem::Template(Template::Superscript),
                PaletteItem::Template(Template::Subscript),
            ],
            &[
                PaletteItem::Template(Template::LimitAbove),
                PaletteItem::Template(Template::LimitBelow),
            ],
        ],
    },
    // 6. large operators
    Palette {
        name: "求和与积分",
        columns: &[
            &[
                PaletteItem::Template(Template::Sum),
                PaletteItem::Template(Template::Product),
                PaletteItem::Template(Template::Coproduct),
            ],
            &[
                PaletteItem::Template(Template::Integral),
                PaletteItem::Template(Template::DoubleIntegral),
                PaletteItem::Template(Template::TripleIntegral),
            ],
            &[
                PaletteItem::Template(Template::ContourIntegral),
                PaletteItem::Template(Template::LimitAbove),
            ],
        ],
    },
    // 7. bars, braces and accents
    Palette {
        name: "上下划线、括号与重音",
        columns: &[
            &[
                PaletteItem::Template(Template::OverBar),
                PaletteItem::Template(Template::UnderBar),
                PaletteItem::Template(Template::OverBrace),
                PaletteItem::Template(Template::UnderBrace),
            ],
            &[
                PaletteItem::Template(Template::OverArrow),
                PaletteItem::Template(Template::UnderArrow),
                PaletteItem::Template(Template::Accent(AccentKind::Hat)),
                PaletteItem::Template(Template::Accent(AccentKind::Tilde)),
            ],
            &[
                PaletteItem::Template(Template::Accent(AccentKind::Dot)),
                PaletteItem::Template(Template::Accent(AccentKind::DoubleDot)),
                PaletteItem::Template(Template::Accent(AccentKind::Check)),
                PaletteItem::Template(Template::Accent(AccentKind::Breve)),
            ],
            &[
                PaletteItem::Template(Template::Accent(AccentKind::WideHat)),
                PaletteItem::Template(Template::Accent(AccentKind::Vector)),
                PaletteItem::Template(Template::Accent(AccentKind::Strike)),
                PaletteItem::Template(Template::Accent(AccentKind::Circle)),
            ],
        ],
    },
    // 8. fences
    Palette {
        name: "括号",
        columns: &[
            &[
                PaletteItem::Template(Template::Fence(Delim::Paren, Delim::Paren)),
                PaletteItem::Template(Template::Fence(Delim::Bracket, Delim::Bracket)),
                PaletteItem::Template(Template::Fence(Delim::Brace, Delim::Brace)),
                PaletteItem::Template(Template::Fence(Delim::Abs, Delim::Abs)),
            ],
            &[
                PaletteItem::Template(Template::Fence(Delim::Norm, Delim::Norm)),
                PaletteItem::Template(Template::Fence(Delim::Angle, Delim::Angle)),
                PaletteItem::Template(Template::Fence(Delim::Floor, Delim::Floor)),
                PaletteItem::Template(Template::Fence(Delim::Ceil, Delim::Ceil)),
            ],
            &[
                PaletteItem::Template(Template::Fence(Delim::Paren, Delim::None)),
                PaletteItem::Template(Template::Fence(Delim::None, Delim::Paren)),
                PaletteItem::Template(Template::Fence(Delim::Brace, Delim::None)),
                PaletteItem::Template(Template::Fence(Delim::None, Delim::Brace)),
            ],
        ],
    },
    // 9. matrices and cases
    Palette {
        name: "矩阵与分段函数",
        columns: &[
            &[
                PaletteItem::Template(Template::Matrix {
                    rows: 2,
                    cols: 2,
                    kind: MatrixKind::Matrix,
                    delims: Some((Delim::Paren, Delim::Paren)),
                }),
                PaletteItem::Template(Template::Matrix {
                    rows: 2,
                    cols: 2,
                    kind: MatrixKind::Matrix,
                    delims: Some((Delim::Bracket, Delim::Bracket)),
                }),
            ],
            &[
                PaletteItem::Template(Template::Matrix {
                    rows: 3,
                    cols: 3,
                    kind: MatrixKind::Matrix,
                    delims: Some((Delim::Paren, Delim::Paren)),
                }),
                PaletteItem::Template(Template::Matrix {
                    rows: 2,
                    cols: 1,
                    kind: MatrixKind::Cases,
                    delims: Some((Delim::Brace, Delim::None)),
                }),
            ],
            &[
                PaletteItem::Template(Template::Matrix {
                    rows: 2,
                    cols: 1,
                    kind: MatrixKind::Aligned,
                    delims: None,
                }),
                PaletteItem::Template(Template::Pile),
            ],
            &[
                PaletteItem::Template(Template::Matrix {
                    rows: 2,
                    cols: 3,
                    kind: MatrixKind::Array,
                    delims: None,
                }),
                PaletteItem::Template(Template::Matrix {
                    rows: 1,
                    cols: 2,
                    kind: MatrixKind::Matrix,
                    delims: Some((Delim::Abs, Delim::Abs)),
                }),
            ],
        ],
    },
    // 10. function names
    Palette {
        name: "函数",
        columns: &[
            &[
                PaletteItem::Template(Template::Function("sin")),
                PaletteItem::Template(Template::Function("cos")),
                PaletteItem::Template(Template::Function("tan")),
            ],
            &[
                PaletteItem::Template(Template::Function("cot")),
                PaletteItem::Template(Template::Function("sec")),
                PaletteItem::Template(Template::Function("csc")),
            ],
            &[
                PaletteItem::Template(Template::Function("arcsin")),
                PaletteItem::Template(Template::Function("arccos")),
                PaletteItem::Template(Template::Function("arctan")),
            ],
            &[
                PaletteItem::Template(Template::Function("sinh")),
                PaletteItem::Template(Template::Function("cosh")),
                PaletteItem::Template(Template::Function("tanh")),
            ],
            &[
                PaletteItem::Template(Template::Function("log")),
                PaletteItem::Template(Template::Function("ln")),
                PaletteItem::Template(Template::Function("exp")),
            ],
        ],
    },
    // 11. explicit spacing
    Palette {
        name: "间距",
        columns: &[
            &[
                PaletteItem::Space(Space::Thin),
                PaletteItem::Space(Space::Medium),
                PaletteItem::Space(Space::Thick),
            ],
            &[
                PaletteItem::Space(Space::Quad),
                PaletteItem::Space(Space::DoubleQuad),
                PaletteItem::Space(Space::NegativeThin),
            ],
        ],
    },
];

/// Explicit spacing commands, kept apart from the palettes because they have no glyph.
pub fn spacing_commands() -> &'static [(&'static str, Space)] {
    &[
        ("thin space", Space::Thin),
        ("medium space", Space::Medium),
        ("thick space", Space::Thick),
        ("quad", Space::Quad),
        ("double quad", Space::DoubleQuad),
        ("negative thin space", Space::NegativeThin),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_palette_is_populated_and_has_consistent_columns() {
        let palettes = palettes();
        assert_eq!(palettes.len(), 11);
        for palette in palettes {
            assert!(!palette.name.is_empty());
            assert!(
                palette.columns.len() >= 2,
                "{} has too few columns",
                palette.name
            );
            for column in palette.columns {
                assert!(!column.is_empty(), "{} has an empty column", palette.name);
            }
        }
    }

    #[test]
    fn every_template_item_builds_a_node_with_the_expected_slot_count() {
        for palette in palettes() {
            for column in palette.columns {
                for item in *column {
                    if let PaletteItem::Template(template) = item {
                        let (node, entry) = template.build();
                        assert!(
                            node.slot_count() > 0,
                            "{} builds an empty node",
                            template.name()
                        );
                        assert!(!entry.is_empty(), "{} has no entry slot", template.name());
                        for slot in &entry {
                            assert!(
                                *slot < node.slot_count(),
                                "{} entry slot {slot} is out of range",
                                template.name()
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn binary_and_relation_symbols_carry_the_right_class() {
        let items: Vec<PaletteItem> = palettes()[0]
            .columns
            .iter()
            .flat_map(|column| column.iter().copied())
            .collect();
        assert!(items.contains(&PaletteItem::symbol('\u{00D7}', Class::Bin)));
        assert!(items.contains(&PaletteItem::symbol('\u{2264}', Class::Rel)));
        assert!(items.contains(&PaletteItem::symbol('\u{2282}', Class::Rel)));
    }

    #[test]
    fn palette_labels_are_non_empty() {
        for palette in palettes() {
            for column in palette.columns {
                for item in *column {
                    assert!(!item.label().is_empty());
                }
            }
        }
    }

    #[test]
    fn spacing_commands_cover_the_negative_case() {
        let commands = spacing_commands();
        assert!(
            commands
                .iter()
                .any(|(_, space)| *space == Space::NegativeThin)
        );
        assert_eq!(commands.len(), 6);
    }

    #[test]
    fn spacing_entries_label_their_width() {
        let label = PaletteItem::Space(Space::NegativeThin).label();
        assert_eq!(label, "-3/18 space");
        assert_eq!(PaletteItem::Space(Space::Quad).label(), "18/18 space");
    }
}
