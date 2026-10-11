//! Style-preset application: how a `LayoutStyle` built from a preset changes the same tree.

use mt_core::{Class, Math, Node};
use mt_layout::{layout, math_variant, ApproxMetrics, Box as LBox, LayoutStyle, StyleFonts};

/// Width of the laid-out `math`, in points.
fn width(math: &Math, style: &LayoutStyle) -> f64 {
    layout(math, &ApproxMetrics, style).width()
}

/// The first glyph box of a laid-out equation.
fn first_glyph(node: &LBox) -> (char, mt_core::Font) {
    let mut found = None;
    node.visit(&mut |part| {
        if found.is_none() {
            if let LBox::Glyph { ch, font, .. } = part {
                found = Some((*ch, *font));
            }
        }
    });
    found.expect("at least one glyph")
}

#[test]
fn default_style_typesets_like_the_engine_always_has() {
    let math = Math::new(Node::row([
        Node::variable('a'),
        Node::symbol('=', Class::Rel),
        Node::variable('b'),
    ]));
    let plain = LayoutStyle::display(10.0);
    let mut with_tables = plain;
    with_tables.spacing = mt_layout::Spacing::default();
    with_tables.fonts = StyleFonts::default();
    assert_eq!(width(&math, &plain), width(&math, &with_tables));
}

#[test]
fn a_bigger_thick_space_widens_a_relation() {
    let math = Math::new(Node::row([
        Node::variable('a'),
        Node::symbol('=', Class::Rel),
        Node::variable('b'),
    ]));
    let mut wide = LayoutStyle::display(10.0);
    wide.spacing.thick_kern = mt_layout::constants::THICK_KERN * 2.0;
    assert!(
        width(&math, &wide) > width(&math, &LayoutStyle::display(10.0)) + 2.0 * wide.spacing.thick_kern,
        "two thick kerns must grow the row"
    );
}

#[test]
fn tighter_fraction_gaps_shrink_a_fraction() {
    let math = Math::new(Node::Frac(Box::new(mt_core::Frac::new(
        Node::variable('a'),
        Node::variable('b'),
    ))));
    let mut tight = LayoutStyle::display(10.0);
    tight.spacing.frac_rule_gap = 0.0;
    let (plain, squeezed) = (
        layout(&math, &ApproxMetrics, &LayoutStyle::display(10.0)),
        layout(&math, &ApproxMetrics, &tight),
    );
    assert!(
        plain.height() + plain.depth() > squeezed.height() + squeezed.depth(),
        "a fraction without rule gap must be shorter"
    );
}

#[test]
fn the_variable_slot_reletters_the_glyph() {
    let math = Math::new(Node::variable('x'));
    let mut scripted = LayoutStyle::display(10.0);
    scripted.fonts.variable = mt_core::Font::Script;
    let (ch, font) = first_glyph(&layout(&math, &ApproxMetrics, &scripted));
    assert_eq!(font, mt_core::Font::Script);
    assert_eq!(ch, math_variant('x', mt_core::Font::Script));

    // The default slot keeps the classic italic face, drawn with its variant codepoint.
    let (ch, font) = first_glyph(&layout(&math, &ApproxMetrics, &LayoutStyle::display(10.0)));
    assert_eq!(font, mt_core::Font::Italic);
    assert_eq!(ch, math_variant('x', mt_core::Font::Italic));
    assert_ne!(ch, 'x', "the italic variable must not draw as upright x");
}

#[test]
fn number_and_greek_slots_restyle_their_roles() {
    let math = Math::new(Node::row([Node::char_with('7', Class::Ord, mt_core::Font::Upright), Node::char_with('α', Class::Ord, mt_core::Font::Italic)]));
    let mut bold = LayoutStyle::display(10.0);
    bold.fonts.number = mt_core::Font::Bold;
    bold.fonts.greek = mt_core::Font::Upright;

    let mut glyphs = Vec::new();
    layout(&math, &ApproxMetrics, &bold).visit(&mut |part| {
        if let LBox::Glyph { ch, font, .. } = part {
            glyphs.push((*ch, *font));
        }
    });
    assert_eq!(glyphs[0], (math_variant('7', mt_core::Font::Bold), mt_core::Font::Bold));
    assert_eq!(glyphs[1], ('α', mt_core::Font::Upright));
}

#[test]
fn explicit_faces_survive_any_slot() {
    let math = Math::new(Node::row([
        Node::char_with('x', Class::Ord, mt_core::Font::Fraktur),
        Node::char_with('5', Class::Ord, mt_core::Font::SansSerif),
    ]));
    let mut odd = LayoutStyle::display(10.0);
    odd.fonts.variable = mt_core::Font::BoldScript;
    odd.fonts.number = mt_core::Font::DoubleStruck;
    let mut glyphs = Vec::new();
    layout(&math, &ApproxMetrics, &odd).visit(&mut |part| {
        if let LBox::Glyph { ch, font, .. } = part {
            glyphs.push((*ch, *font));
        }
    });
    assert_eq!(glyphs[0], (math_variant('x', mt_core::Font::Fraktur), mt_core::Font::Fraktur));
    assert_eq!(glyphs[1], (math_variant('5', mt_core::Font::SansSerif), mt_core::Font::SansSerif));
}

#[test]
fn text_runs_follow_the_text_slot_not_the_variable_rules() {
    let math = Math::new(Node::row([
        Node::text("sin"),
        Node::variable('x'),
    ]));
    let mut upright_variables = LayoutStyle::display(10.0);
    upright_variables.fonts.variable = mt_core::Font::Upright;
    upright_variables.fonts.text = mt_core::Font::Bold;
    let mut glyphs = Vec::new();
    layout(&math, &ApproxMetrics, &upright_variables).visit(&mut |part| {
        if let LBox::Glyph { ch, font, .. } = part {
            glyphs.push((*ch, *font));
        }
    });
    assert_eq!(glyphs[0], (math_variant('s', mt_core::Font::Bold), mt_core::Font::Bold));
    assert_eq!(glyphs[3], (math_variant('x', mt_core::Font::Upright), mt_core::Font::Upright));
}

#[test]
fn styled_overrides_win_over_the_slots() {
    let math = Math::new(Node::Styled(Box::new(mt_core::Styled {
        body: Node::variable('x'),
        style: None,
        font: Some(mt_core::Font::Fraktur),
        class: None,
    })));
    let mut bold = LayoutStyle::display(10.0);
    bold.fonts.variable = mt_core::Font::Bold;
    let (ch, font) = first_glyph(&layout(&math, &ApproxMetrics, &bold));
    assert_eq!((ch, font), (math_variant('x', mt_core::Font::Fraktur), mt_core::Font::Fraktur));
}
