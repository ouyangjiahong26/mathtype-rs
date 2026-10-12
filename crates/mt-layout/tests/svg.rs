//! Preview tests: the SVG `to_svg` emits.

use mt_core::{Class, Delim, Fence, Frac, Math, Matrix, Node, Scripts, Sqrt};
use mt_layout::{
    ApproxMetrics, Box as LBox, LayoutStyle, constants as c, layout, to_svg, view_box,
};

fn style() -> LayoutStyle {
    LayoutStyle::display(10.0)
}

fn boxes(body: Node) -> LBox {
    layout(&Math::new(body), &ApproxMetrics, &style())
}

fn svg(body: Node) -> String {
    to_svg(&boxes(body), &style())
}

/// The four numbers of the `viewBox` attribute.
#[track_caller]
fn parse_view_box(svg: &str) -> (f64, f64, f64, f64) {
    let start = svg.find("viewBox=\"").expect("a viewBox attribute") + "viewBox=\"".len();
    let rest = &svg[start..];
    let end = rest.find('"').expect("a closing quote");
    let parts: Vec<f64> = rest[..end]
        .split_whitespace()
        .map(|part| part.parse().expect("a number in the viewBox"))
        .collect();
    assert_eq!(
        parts.len(),
        4,
        "viewBox needs four numbers, got {:?}",
        rest[..end].to_owned()
    );
    assert!(parts.iter().all(|value| value.is_finite()));
    assert!(
        parts[2] > 0.0 && parts[3] > 0.0,
        "the view box needs a positive size"
    );
    (parts[0], parts[1], parts[2], parts[3])
}

#[test]
fn a_mixed_equation_previews_one_text_element_per_glyph() {
    // \frac{a}{b} + \sqrt{x_i^2}
    let body = Node::row([
        Node::Frac(Box::new(Frac::new(
            Node::variable('a'),
            Node::variable('b'),
        ))),
        Node::symbol('+', Class::Bin),
        Node::Sqrt(Box::new(Sqrt::square(Node::Scripts(Box::new(
            Scripts::attach(
                Node::variable('x'),
                Some(Node::variable('i')),
                Some(Node::variable('2')),
            ),
        ))))),
    ]);
    let boxes = boxes(body);
    let svg = to_svg(&boxes, &style());

    // a, b, +, the radical sign, x, i, 2.
    assert_eq!(svg.matches("<text").count(), 7);
    assert_eq!(svg.matches("</text>").count(), 7);
    assert_eq!(
        svg.matches("<rect").count(),
        2,
        "the fraction rule and the radical's overbar"
    );
    assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
    assert!(svg.trim_end().ends_with("</svg>"));
    assert!(svg.contains("font-family=\"Latin Modern Math, STIX Two Math, Cambria Math, serif\""));
    assert!(
        svg.ends_with("</svg>\n"),
        "a trailing newline closes the document"
    );

    let (x, y, width, height) = parse_view_box(&svg);
    let expected = view_box(&boxes, &style());
    for (actual, expected) in [
        (x, expected.0),
        (y, expected.1),
        (width, expected.2),
        (height, expected.3),
    ] {
        assert!(
            (actual - expected).abs() < 1e-3,
            "the viewBox matches the root box: {actual} vs {expected}"
        );
    }
    assert!(width > boxes.width());
    assert!(height > boxes.height() + boxes.depth());
}

#[test]
fn the_view_box_covers_the_root_box_plus_padding() {
    let boxes = boxes(Node::variable('x'));
    let (_, _, width, height) = view_box(&boxes, &style());
    let padding = c::SVG_PADDING * 10.0;
    assert!((width - (boxes.width() + 2.0 * padding)).abs() < 1e-9);
    assert!((height - (boxes.height() + boxes.depth() + 2.0 * padding)).abs() < 1e-9);
}

#[test]
fn text_is_escaped_and_faces_are_marked() {
    let svg = svg(Node::row([
        Node::text("<&>"),
        Node::variable('x'),
        Node::Styled(Box::new(mt_core::Styled {
            body: Node::variable('n'),
            style: None,
            font: Some(mt_core::Font::Bold),
            class: None,
        })),
    ]));
    // Each glyph is its own text element, so the three escaped characters stand apart.
    assert!(svg.contains("&lt;"), "less-than is escaped");
    assert!(svg.contains("&amp;"), "ampersand is escaped");
    assert!(svg.contains("&gt;"), "greater-than is escaped");
    assert!(!svg.contains(">&lt;&amp;&gt;<"));
    assert_eq!(svg.matches("<text").count(), 5);
    assert!(
        svg.contains("font-style=\"italic\""),
        "italic variables are marked"
    );
    assert!(svg.contains("font-weight=\"bold\""), "bold runs are marked");
}

#[test]
fn empty_slots_are_drawn_as_dashed_rectangles() {
    let svg = svg(Node::Frac(Box::new(Frac::new(
        Node::empty(),
        Node::variable('x'),
    ))));
    assert!(svg.contains("fill=\"none\" stroke=\"#888888\""));
    assert!(svg.contains("stroke-dasharray=\"1 1\""));
    // The fraction's rule is filled rather than stroked.
    assert!(svg.contains("fill=\"black\""));
}

#[test]
fn rules_are_drawn_as_rectangles_at_their_rule_position() {
    let math = Math::new(Node::Frac(Box::new(Frac::new(
        Node::variable('a'),
        Node::variable('b'),
    ))));
    let boxes = layout(&math, &ApproxMetrics, &style());
    let svg = to_svg(&boxes, &style());
    // The fraction rule is 0.4 points thick and centred on the axis, 2.5 points up; its two
    // full-size arguments are 5 points wide.
    assert!(svg.contains("y=\"-2.7\""), "the rule's top edge: {svg}");
    assert!(svg.contains("height=\"0.4\""));
    assert!(svg.contains("width=\"5\""));
}

#[test]
fn stretched_glyphs_carry_a_vertical_scale() {
    let mut matrix = Matrix::empty(4, 1);
    for row in &mut matrix.rows {
        row[0] = Node::symbol('\u{222B}', Class::Op);
    }
    let svg = svg(Node::Fence(Box::new(Fence {
        open: Delim::Paren,
        close: Delim::Paren,
        body: Node::Matrix(Box::new(matrix)),
    })));
    assert!(
        svg.contains("scale(1 1.76"),
        "the stretched delimiter is scaled: {svg}"
    );
    assert!(svg.contains("font-size=\"28\""));
}
