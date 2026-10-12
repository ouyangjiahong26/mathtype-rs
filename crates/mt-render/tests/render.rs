//! Output tests: real properties of the PNG and EPS a laid-out equation produces.

use mt_core::{Class, Font, Math, Node, Scripts};
use mt_layout::{ApproxMetrics, Box as LBox, LayoutStyle, layout};
use mt_render::{
    Image, RenderError, RenderOptions, image_bounds, image_size, render, render_eps, render_png,
};

const FONT: &[u8] = include_bytes!("../../../assets/fonts/latinmodern-math.otf");

fn face() -> ttf_parser::Face<'static> {
    ttf_parser::Face::parse(FONT, 0).expect("the bundled font parses")
}

fn style() -> LayoutStyle {
    LayoutStyle::display(10.0)
}

/// An upright digit.
fn digit(d: char) -> Node {
    Node::char_with(d, Class::Ord, Font::Upright)
}

/// `base` with a superscript.
fn superscript(base: Node, sup: Node) -> Node {
    Node::Scripts(Box::new(Scripts {
        base,
        sub: None,
        sup: Some(sup),
    }))
}

/// Lay out a body at full size with the approximate metrics.
fn boxes(body: Node) -> LBox {
    layout(&Math::new(body), &ApproxMetrics, &style())
}

/// `x^2 + y^2 = z^2`, a mixed equation with scripts, a binary operator and a relation.
fn sample() -> LBox {
    boxes(Node::row([
        superscript(Node::variable('x'), digit('2')),
        Node::symbol('+', Class::Bin),
        superscript(Node::variable('y'), digit('2')),
        Node::symbol('=', Class::Rel),
        superscript(Node::variable('z'), digit('2')),
    ]))
}

/// A single scripted variable.
fn scripted() -> LBox {
    boxes(superscript(Node::variable('x'), digit('2')))
}

/// Dimensions in the PNG IHDR chunk.
fn png_dimensions(png: &[u8]) -> (u32, u32) {
    assert_eq!(
        &png[..8],
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
        "PNG magic"
    );
    assert_eq!(&png[12..16], b"IHDR", "first chunk is IHDR");
    let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
    (width, height)
}

/// Alpha of the pixel at `(x, y)`.
fn alpha(image: &Image, x: u32, y: u32) -> u8 {
    image.rgba[(y as usize * image.width as usize + x as usize) * 4 + 3]
}

/// The leftmost column that carries ink.
fn first_inked_column(image: &Image) -> Option<u32> {
    for x in 0..image.width {
        for y in 0..image.height {
            if alpha(image, x, y) > 0 {
                return Some(x);
            }
        }
    }
    None
}

#[test]
fn png_has_magic_and_header_dimensions_matching_the_image() {
    let face = face();
    let tree = sample();
    let options = RenderOptions {
        scale: 2.0,
        ..RenderOptions::default()
    };
    let png = render_png(&tree, &style(), &face, &options).expect("PNG encodes");

    let (width, height) = png_dimensions(&png);
    let image = render(&tree, &style(), &face, &options);
    assert_eq!((width, height), (image.width, image.height));
    assert_eq!(
        (width, height),
        image_size(&tree, &style(), &face, &options)
    );
    assert!(
        width > 12 && height > 12,
        "the equation occupies a real image: {width}x{height}"
    );
}

#[test]
fn a_glyph_puts_ink_inside_the_box_and_none_in_the_corner() {
    let face = face();
    let tree = boxes(Node::variable('x'));
    let options = RenderOptions {
        scale: 3.0,
        ..RenderOptions::default()
    };
    let image = render(&tree, &style(), &face, &options);

    let inked = image.rgba.chunks_exact(4).filter(|p| p[3] > 0).count();
    assert!(
        inked > 10,
        "the glyph leaves a real mark, got {inked} pixels"
    );

    // The top-left corner sits in the padding, away from every glyph.
    assert_eq!(image.rgba[3], 0, "the corner is background");
}

#[test]
fn scale_multiplies_the_output_dimensions() {
    let face = face();
    let tree = boxes(Node::row([
        Node::variable('x'),
        Node::symbol('+', Class::Bin),
        digit('1'),
    ]));
    let (x0, y0, x1, y1) = image_bounds(&tree, &style(), &face, &RenderOptions::default());
    let (width, height) = (x1 - x0, y1 - y0);

    let small = render(
        &tree,
        &style(),
        &face,
        &RenderOptions {
            scale: 1.0,
            ..RenderOptions::default()
        },
    );
    let large = render(
        &tree,
        &style(),
        &face,
        &RenderOptions {
            scale: 2.0,
            ..RenderOptions::default()
        },
    );

    assert_eq!(small.width as f64, width.ceil());
    assert_eq!(small.height as f64, height.ceil());
    assert_eq!(large.width as f64, (width * 2.0).ceil());
    assert_eq!(large.height as f64, (height * 2.0).ceil());
    assert!(large.width >= small.width * 2 - 1 && large.height >= small.height * 2 - 1);
}

#[test]
fn a_rule_renders_as_a_filled_band() {
    let face = face();
    let rule = LBox::Rule {
        x: 5.0,
        y: 0.0,
        width: 10.0,
        height: 1.0,
        depth: 1.0,
        dashed: false,
    };
    let tree = LBox::HBox {
        items: vec![rule],
        width: 20.0,
        height: 1.0,
        depth: 1.0,
    };
    let options = RenderOptions {
        scale: 4.0,
        ..RenderOptions::default()
    };
    let image = render(&tree, &style(), &face, &options);

    let (bx0, by0, _, _) = image_bounds(&tree, &style(), &face, &options);
    // Bottom edge of the band in pixels; the band spans rows [top, bottom).
    let band_top = ((0.0 - 1.0 - by0) * 4.0) as u32;
    let band_bottom = ((0.0 + 1.0 - by0) * 4.0) as u32;
    assert!(band_bottom > band_top);
    let middle = (band_top + band_bottom) / 2;
    let column = ((5.0 + 5.0 + 0.0 - bx0) * 4.0) as u32; // interior of the rule

    assert_eq!(
        alpha(&image, column, middle),
        255,
        "inside the band is filled"
    );
    assert!(
        band_top >= 2 && alpha(&image, column, band_top - 2) == 0,
        "above the band is empty"
    );
}

#[test]
fn a_dashed_rule_leaves_gaps() {
    let face = face();
    let rule = LBox::Rule {
        x: 5.0,
        y: 0.0,
        width: 10.0,
        height: 1.0,
        depth: 1.0,
        dashed: true,
    };
    let tree = LBox::HBox {
        items: vec![rule],
        width: 20.0,
        height: 1.0,
        depth: 1.0,
    };
    let options = RenderOptions {
        scale: 4.0,
        ..RenderOptions::default()
    };
    let image = render(&tree, &style(), &face, &options);

    let (bx0, _, _, _) = image_bounds(&tree, &style(), &face, &options);
    let col0 = ((5.0 - bx0) * 4.0) as u32;
    let col1 = ((15.0 - bx0) * 4.0) as u32;

    // Some row along the outline must carry both dashes and the gaps between them.
    let mut row_with_gaps = None;
    for y in 0..image.height {
        let mut inked = 0u32;
        let mut empty = 0u32;
        for x in col0..col1 {
            if alpha(&image, x, y) > 0 {
                inked += 1;
            } else {
                empty += 1;
            }
        }
        if inked > 0 && empty > 0 {
            row_with_gaps = Some((y, inked, empty));
            break;
        }
    }
    let (y, inked, empty) = row_with_gaps.expect("a dashed outline has gaps");
    assert!(
        inked >= 2 && empty >= 2,
        "row {y} has {inked} ink and {empty} gaps"
    );
}

#[test]
fn eps_contains_the_required_structures() {
    let face = face();
    let tree = scripted();
    let eps = render_eps(&tree, &style(), &face, &RenderOptions::default());

    assert!(eps.starts_with("%!PS-Adobe-3.0 EPSF-3.0"), "EPS header");
    assert!(eps.contains("%%BoundingBox:"), "integer bounding box");
    assert!(eps.contains("moveto"), "path start");
    assert!(eps.contains("curveto"), "outline curves");
    assert!(eps.contains("showpage"), "page trailer");
    assert!(eps.contains("%%EOF"), "EPS terminator");

    // Glyph outlines are emitted as Bézier paths, so curves follow the `moveto`s.
    let moves = eps.matches("moveto").count();
    let curves = eps.matches("curveto").count();
    assert!(moves >= 1, "at least one contour: {moves}");
    assert!(curves >= 1, "at least one outline curve: {curves}");
}

#[test]
fn eps_bounding_box_matches_the_image_box() {
    let face = face();
    let tree = sample();
    let (x0, y0, x1, y1) = image_bounds(&tree, &style(), &face, &RenderOptions::default());
    let eps = render_eps(&tree, &style(), &face, &RenderOptions::default());

    let line = eps
        .lines()
        .find(|line| line.starts_with("%%BoundingBox:"))
        .expect("a %%BoundingBox line");
    let values: Vec<i64> = line["%%BoundingBox:".len()..]
        .split_whitespace()
        .map(|part| part.parse().expect("an integer"))
        .collect();
    assert_eq!(values.len(), 4);
    assert_eq!(values[0], 0);
    assert_eq!(values[1], 0);
    assert_eq!(values[2] as f64, (x1 - x0).ceil(), "width in points");
    assert_eq!(values[3] as f64, (y1 - y0).ceil(), "height in points");
}

#[test]
fn eps_rule_rectangle_spans_the_rule_box() {
    let face = face();
    let rule = LBox::Rule {
        x: 5.0,
        y: 0.0,
        width: 10.0,
        height: 1.0,
        depth: 1.0,
        dashed: false,
    };
    let tree = LBox::HBox {
        items: vec![rule],
        width: 20.0,
        height: 1.0,
        depth: 1.0,
    };
    let options = RenderOptions::default();
    let (bx0, _, _, by1) = image_bounds(&tree, &style(), &face, &options);
    let eps = render_eps(&tree, &style(), &face, &options);

    // The rule is filled as `newpath <llx> <lly> <width> <height> rectfill`, after the y flip.
    let line = eps
        .lines()
        .find(|line| line.ends_with("rectfill"))
        .expect("the rule is filled with a rectangle");
    let values: Vec<f64> = line
        .split_whitespace()
        .filter_map(|token| token.parse().ok())
        .collect();
    assert_eq!(values.len(), 4, "a corner, width and height: {line}");
    let (llx, lly, width, height) = (values[0], values[1], values[2], values[3]);

    // The rectangle runs from the depth edge (layout y + depth) up to the top edge (y - height).
    assert!(
        (llx - (5.0 - bx0)).abs() < 1e-3,
        "left edge at {llx}, expected {}",
        5.0 - bx0
    );
    assert!((width - 10.0).abs() < 1e-3, "width {width}");
    assert!(
        height > 0.0,
        "the rectangle height must be positive, got {height}"
    );
    assert!(
        (height - 2.0).abs() < 1e-3,
        "height spans -1..1 in layout points, got {height}"
    );
    assert!(
        (lly - (by1 - 1.0)).abs() < 1e-3,
        "lower edge at {lly}, expected {}",
        by1 - 1.0
    );
    assert!(
        (lly + height - (by1 + 1.0)).abs() < 1e-3,
        "upper edge at {}",
        lly + height
    );
}

#[test]
fn an_empty_hbox_still_renders_a_valid_tiny_image() {
    let face = face();
    let tree = LBox::HBox {
        items: vec![],
        width: 0.0,
        height: 0.0,
        depth: 0.0,
    };
    let options = RenderOptions::default();
    let image = render(&tree, &style(), &face, &options);

    // Six points of padding each way, rounded up at scale one.
    assert_eq!((image.width, image.height), (6, 6));
    assert_eq!(image.rgba.len(), 6 * 6 * 4);
    assert!(
        image.rgba.iter().all(|&byte| byte == 0),
        "transparent background"
    );

    let png = render_png(&tree, &style(), &face, &options).expect("PNG encodes");
    assert_eq!(png_dimensions(&png), (6, 6));

    // An opaque background fills every pixel.
    let opaque = RenderOptions {
        background: Some([255, 0, 0, 255]),
        ..RenderOptions::default()
    };
    let filled = render(&tree, &style(), &face, &opaque);
    assert!(
        filled
            .rgba
            .chunks_exact(4)
            .all(|pixel| pixel == [255, 0, 0, 255])
    );

    // EPS is valid too.
    let eps = render_eps(&tree, &style(), &face, &options);
    assert!(eps.contains("%%BoundingBox: 0 0 6 6"));
    assert!(eps.contains("showpage"));
}

#[test]
fn a_glyph_at_ten_points_starts_near_that_column() {
    let face = face();
    let glyph = LBox::Glyph {
        ch: 'x',
        font: Font::Italic,
        x: 10.0,
        y: 0.0,
        size: 10.0,
        yscale: 1.0,
    };
    let tree = LBox::HBox {
        items: vec![glyph],
        width: 15.0,
        height: 5.0,
        depth: 3.0,
    };
    let options = RenderOptions::default(); // scale 1, padding 0.3 em = 3 pt
    let image = render(&tree, &style(), &face, &options);

    let (bx0, ..) = image_bounds(&tree, &style(), &face, &options);
    let expected = (10.0 - bx0).round() as i64;
    let left = first_inked_column(&image).expect("the glyph is drawn") as i64;
    assert!(
        (left - expected).abs() <= 4,
        "leftmost ink at column {left}, expected near {expected}"
    );

    // Sanity: the ink really is on the right side of the image, not at the origin.
    assert!(
        left > (0.0 - bx0) as i64 + 5,
        "the glyph is not drawn at the padding edge"
    );
}

#[test]
fn glyph_ink_beyond_the_advance_grows_the_image() {
    let face = face();
    let glyph = LBox::Glyph {
        ch: 'x',
        font: Font::Italic,
        x: 0.0,
        y: 0.0,
        size: 10.0,
        yscale: 1.0,
    };
    // A deliberately too-narrow enclosing box: the glyph is wider than its advance.
    let tree = LBox::HBox {
        items: vec![glyph],
        width: 1.0,
        height: 1.0,
        depth: 1.0,
    };
    let options = RenderOptions::default();
    let pad = options.padding_em * style().size;

    let (_, _, x1, _) = image_bounds(&tree, &style(), &face, &options);
    assert!(
        x1 > 1.0 + pad,
        "the ink box grew past the advance, x1 = {x1}"
    );

    let image = render(&tree, &style(), &face, &options);
    let mut rightmost = 0;
    for x in 0..image.width {
        for y in 0..image.height {
            if alpha(&image, x, y) > 0 {
                rightmost = x;
            }
        }
    }
    assert!(
        rightmost > 3,
        "the overflowing glyph was still drawn, rightmost = {rightmost}"
    );
    assert!(rightmost < image.width, "the ink stayed inside the image");
}

#[test]
fn render_error_displays() {
    let error = RenderError::Png("boom".to_owned());
    assert!(error.to_string().contains("boom"));
}
