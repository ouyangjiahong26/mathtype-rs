//! Caret geometry tests: the row anchors `layout_with_anchors` reports.

use mt_core::{Class, Frac, FracKind, Math, Matrix, Node, Path, Scripts, Space, Sqrt};
use mt_layout::{
    constants as c, layout, layout_with_anchors, ApproxMetrics, Box as LBox, LayoutStyle, RowAnchor,
};

fn style() -> LayoutStyle {
    LayoutStyle::display(10.0)
}

fn lay(body: Node) -> LBox {
    layout(&Math::new(body), &ApproxMetrics, &style())
}

#[track_caller]
fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "expected {expected}, got {actual}");
}

#[track_caller]
fn anchor<'a>(anchors: &'a [RowAnchor], path: &Path) -> &'a RowAnchor {
    anchors
        .iter()
        .find(|anchor| &anchor.path == path)
        .unwrap_or_else(|| panic!("no anchor for path {path}"))
}

#[test]
fn the_root_row_reports_one_offset_per_insertion_point() {
    let math = Math::new(Node::row([
        Node::variable('x'),
        Node::symbol('+', Class::Bin),
        Node::variable('n'),
    ]));
    let (boxes, anchors) = layout_with_anchors(&math, &ApproxMetrics, &style());
    // Recording anchors does not change the geometry.
    assert_eq!(boxes, lay(math.body.clone()));
    assert_eq!(anchors.len(), 1);

    let root = anchor(&anchors, &Path::root());
    assert_eq!(root.origin, (0.0, 0.0));
    assert_eq!(root.offsets.len(), 4, "one more insertion point than items");
    close(root.offsets[0], 0.0);
    // The insertion point in front of the binary operator follows its medium kern.
    close(root.offsets[1], 5.0 + c::MEDIUM_KERN * 10.0);
    close(root.offsets[2], 5.0 + 2.0 * c::MEDIUM_KERN * 10.0 + 7.8);
    close(*root.offsets.last().expect("a right edge"), boxes.width());
    assert!(root.offsets.windows(2).all(|pair| pair[1] >= pair[0]));
}

#[test]
fn a_caret_inside_a_fraction_numerator_uses_the_numerator_origin() {
    let math = Math::new(Node::Frac(Box::new(Frac::new(
        Node::row([Node::variable('a')]),
        Node::row([Node::variable('b')]),
    ))));
    let (_, anchors) = layout_with_anchors(&math, &ApproxMetrics, &style());

    // The numerator and the denominator are slots 0 and 1 of the fraction.
    let numerator = anchor(&anchors, &Path::root().child(0));
    let denominator = anchor(&anchors, &Path::root().child(1));
    close(numerator.origin.0, 0.0);
    close(numerator.origin.1, -2.5 - 0.2 - c::FRAC_RULE_GAP * 10.0);
    assert_eq!(numerator.offsets, vec![0.0, 3.5], "the cell is set at script size");
    close(denominator.origin.1, -2.5 + 0.2 + c::FRAC_RULE_GAP * 10.0 + 0.45 * 7.0);
    assert_eq!(denominator.offsets, vec![0.0, 3.5]);
}

#[test]
fn every_row_of_the_tree_gets_exactly_one_anchor() {
    let math = Math::new(Node::row([
        Node::Frac(Box::new(Frac::new(
            Node::row([Node::variable('a'), Node::variable('b')]),
            Node::variable('c'),
        ))),
        Node::Sqrt(Box::new(Sqrt::square(Node::Scripts(Box::new(Scripts::attach(
            Node::variable('x'),
            None,
            Some(Node::variable('2')),
        )))))),
    ]));
    let (_, anchors) = layout_with_anchors(&math, &ApproxMetrics, &style());

    let mut rows = 0;
    math.body.walk(&mut |node| {
        if matches!(node, Node::Row(_)) {
            rows += 1;
        }
    });
    assert_eq!(anchors.len(), rows);
    // The root row and the numerator row; the radical's base and scripts are not rows.
    assert_eq!(anchors.len(), 2);
    assert_eq!(anchors[0].path, Path::root());
    assert_eq!(anchors[1].path, Path::root().child(0).child(0));
    assert_eq!(anchors[1].offsets.len(), 3, "two items, three insertion points");
}

#[test]
fn empty_rows_report_a_single_insertion_point() {
    let math = Math::new(Node::row([Node::variable('x'), Node::empty()]));
    let (_, anchors) = layout_with_anchors(&math, &ApproxMetrics, &style());
    assert_eq!(anchors.len(), 2);
    let empty = anchor(&anchors, &Path::root().child(1));
    close(empty.origin.0, 5.0);
    close(empty.origin.1, 0.0);
    // An empty row has `row.len() == 0`, so the contract `offsets.len() == row.len() + 1` leaves
    // exactly one insertion point, at its left edge. A right edge offset would name caret index
    // `1`, which the caret rejects.
    assert_eq!(empty.offsets, vec![0.0]);
}

#[test]
fn inserting_material_moves_the_later_insertion_points() {
    let before = Math::new(Node::row([Node::variable('x'), Node::variable('n')]));
    let after = Math::new(Node::row([
        Node::variable('x'),
        Node::symbol('+', Class::Bin),
        Node::variable('n'),
    ]));
    let (_, before) = layout_with_anchors(&before, &ApproxMetrics, &style());
    let (_, after) = layout_with_anchors(&after, &ApproxMetrics, &style());
    assert_eq!(before[0].offsets.len(), 3);
    assert_eq!(after[0].offsets.len(), 4);
    // The insertion points in front of the inserted operator are untouched ...
    close(before[0].offsets[0], after[0].offsets[0]);
    close(before[0].offsets[1], 5.0);
    // ... while the ones after it, and the right edge, move right.
    assert!(after[0].offsets[2] > before[0].offsets[1]);
    assert!(after[0].offsets[3] > before[0].offsets[2]);
}

#[test]
fn matrix_cells_are_addressed_row_major() {
    let mut matrix = Matrix::empty(2, 2);
    matrix.rows[0][0] = Node::row([Node::variable('a')]);
    matrix.rows[1][1] = Node::row([Node::variable('d')]);
    let math = Math::new(Node::Matrix(Box::new(matrix)));
    let (_, anchors) = layout_with_anchors(&math, &ApproxMetrics, &style());

    // No row around the table, so the four cells are the first four flat slots of the matrix.
    assert_eq!(anchors.len(), 4);
    let first = anchor(&anchors, &Path::root().child(0));
    let last = anchor(&anchors, &Path::root().child(3));
    close(last.origin.0 - first.origin.0, 10.0);
    assert!(last.origin.1 > first.origin.1, "the second row sits lower");
    assert_eq!(first.offsets, vec![0.0, 5.0]);
    // Each empty cell still draws a placeholder, but reports its single insertion point.
    let empty = anchor(&anchors, &Path::root().child(1));
    assert_eq!(empty.offsets, vec![0.0]);
}

#[test]
fn anchors_stay_inside_the_root_box() {
    let math = Math::new(Node::row([
        Node::variable('x'),
        Node::Space(Space::Thick),
        Node::Frac(Box::new(Frac::new(
            Node::row([Node::variable('a')]),
            Node::row([Node::variable('b')]),
        ))),
    ]));
    let (boxes, anchors) = layout_with_anchors(&math, &ApproxMetrics, &style());
    assert_eq!(anchors.len(), 3);
    for anchor in &anchors {
        let right = anchor.origin.0 + anchor.offsets.last().copied().unwrap_or(0.0);
        assert!(anchor.origin.0 >= -1e-9, "left edge inside the box: {:?}", anchor.origin);
        assert!(right <= boxes.width() + 1e-9, "right edge inside the box: {right}");
        assert!(anchor.origin.1 >= -boxes.height() - 1e-9, "origin below the box top");
        assert!(anchor.origin.1 <= boxes.depth() + 1e-9, "origin above the box bottom");
    }
}

#[test]
fn a_slash_fraction_keeps_row_anchors_unshifted() {
    // Placing the slash through the fraction's own context used to run its `shift_rows` and
    // move every anchor under the fraction a second time, so the anchors must sit exactly
    // where their rows' glyphs landed.
    let mut frac = Frac::new(Node::row([Node::variable('a')]), Node::row([Node::variable('b')]));
    frac.kind = FracKind::Slash;
    let (boxes, anchors) = layout_with_anchors(&Math::new(Node::Frac(Box::new(frac))), &ApproxMetrics, &style());

    fn glyph_at(node: &LBox, ch: char) -> Option<(f64, f64)> {
        match node {
            LBox::Glyph { ch: found, x, y, .. } if *found == ch => Some((*x, *y)),
            LBox::HBox { items, .. } | LBox::VBox { items, .. } => {
                items.iter().find_map(|item| glyph_at(item, ch))
            }
            _ => None,
        }
    }
    let (a_x, a_y) = glyph_at(&boxes, '\u{1D44E}').expect("the italic variable a in the box tree");
    let (b_x, b_y) = glyph_at(&boxes, '\u{1D44F}').expect("the italic variable b in the box tree");

    let numerator = anchor(&anchors, &Path::root().child(0));
    close(numerator.origin.0, a_x);
    close(numerator.origin.1, a_y);
    let denominator = anchor(&anchors, &Path::root().child(1));
    close(denominator.origin.0, b_x);
    close(denominator.origin.1, b_y);
}
