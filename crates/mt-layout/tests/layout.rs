//! Geometry tests: concrete numbers and relations for every construct layout implements.

use mt_core::{
    Accent, AccentKind, BigOp, Class, Delim, Fence, Font, Frac, Limits, Math, MathStyle, Matrix,
    MatrixKind, Node, Scripts, Side, Space, Sqrt, Styled,
};
use mt_layout::{
    ApproxMetrics, Box as LBox, FontMetrics, LayoutStyle, constants as c, layout, math_base,
};

/// A laid-out glyph, flattened out of the box tree for assertions.
#[derive(Clone, Copy, Debug)]
struct Glyph {
    ch: char,
    font: Font,
    x: f64,
    y: f64,
    size: f64,
    yscale: f64,
}

/// A laid-out rule, flattened out of the box tree for assertions.
#[derive(Clone, Copy, Debug)]
struct Rule {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    depth: f64,
    dashed: bool,
}

fn style() -> LayoutStyle {
    LayoutStyle::display(10.0)
}

fn float(value: f64) -> f64 {
    value * 10.0
}

fn lay(node: Node) -> LBox {
    layout(&Math::new(node), &ApproxMetrics, &style())
}

fn walk<'a>(node: &'a LBox, visit: &mut impl FnMut(&'a LBox)) {
    visit(node);
    if let LBox::HBox { items, .. } | LBox::VBox { items, .. } = node {
        for item in items {
            walk(item, visit);
        }
    }
}

fn glyphs(node: &LBox) -> Vec<Glyph> {
    let mut out = Vec::new();
    walk(node, &mut |part| {
        if let LBox::Glyph {
            ch,
            font,
            x,
            y,
            size,
            yscale,
        } = part
        {
            out.push(Glyph {
                ch: *ch,
                font: *font,
                x: *x,
                y: *y,
                size: *size,
                yscale: *yscale,
            });
        }
    });
    out
}

fn glyph(node: &LBox, ch: char) -> Glyph {
    glyphs(node)
        .into_iter()
        .find(|glyph| glyph.ch == ch || math_base(glyph.ch) == Some(ch))
        .unwrap_or_else(|| panic!("no glyph {ch:?} in the box tree"))
}

fn rules(node: &LBox) -> Vec<Rule> {
    let mut out = Vec::new();
    walk(node, &mut |part| {
        if let LBox::Rule {
            x,
            y,
            width,
            height,
            depth,
            dashed,
        } = part
        {
            out.push(Rule {
                x: *x,
                y: *y,
                width: *width,
                height: *height,
                depth: *depth,
                dashed: *dashed,
            });
        }
    });
    out
}

fn kerns(node: &LBox) -> Vec<f64> {
    let mut out = Vec::new();
    walk(node, &mut |part| {
        if let LBox::Kern { width } = part {
            out.push(*width);
        }
    });
    out
}

#[track_caller]
fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "expected {expected}, got {actual}"
    );
}

#[track_caller]
fn close_all(actual: &[f64], expected: &[f64]) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "length mismatch: {actual:?} vs {expected:?}"
    );
    for (actual, expected) in actual.iter().zip(expected) {
        close(*actual, *expected);
    }
}

/// `\frac{a}{b}`.
fn frac_of(numerator: Node, denominator: Node) -> Node {
    Node::Frac(Box::new(Frac::new(numerator, denominator)))
}

// -------------------------------------------------------------------------------------------
// Fractions
// -------------------------------------------------------------------------------------------

#[test]
fn fraction_rule_thickness_gaps_and_axis() {
    let boxes = lay(frac_of(Node::variable('a'), Node::variable('b')));
    let rules = rules(&boxes);
    assert_eq!(rules.len(), 1, "a barred fraction draws exactly one rule");
    let rule = rules[0];
    assert!(!rule.dashed);
    close(rule.x, 0.0);
    // The arguments keep the surrounding size, so both are 5 points wide.
    close(rule.width, 5.0);
    close(rule.height, c::FRAC_RULE_THICKNESS * 10.0 / 2.0);
    close(rule.depth, c::FRAC_RULE_THICKNESS * 10.0 / 2.0);
    close(rule.height + rule.depth, 0.4);
    // The rule sits on the math axis, which ApproxMetrics puts 0.25 em above the baseline.
    close(rule.y, -float(0.25));

    let numerator = glyph(&boxes, 'a');
    let denominator = glyph(&boxes, 'b');
    close(
        numerator.y + 0.0,
        rule.y - rule.height - c::FRAC_RULE_GAP * 10.0,
    );
    close(
        denominator.y - 0.45 * 10.0,
        rule.y + rule.depth + c::FRAC_RULE_GAP * 10.0,
    );
    // Both sides are centred on the widest of the two, and the extents mirror around the axis.
    close(numerator.x, 0.0);
    close(denominator.x, 0.0);
    close(boxes.width(), 5.0);
    close(
        boxes.height(),
        float(0.25) + 0.2 + c::FRAC_RULE_GAP * 10.0 + 0.45 * 10.0,
    );
    close(
        boxes.depth(),
        -float(0.25) + 0.2 + c::FRAC_RULE_GAP * 10.0 + 0.45 * 10.0,
    );
}

#[test]
fn fraction_arguments_keep_the_surrounding_size() {
    let boxes = lay(frac_of(Node::variable('a'), Node::variable('b')));
    close(glyph(&boxes, 'a').size, 10.0);
    close(glyph(&boxes, 'b').size, 10.0);
    // A fraction inside a fraction stays at the surrounding size too; only scripts shrink.
    let nested = lay(frac_of(
        frac_of(Node::variable('a'), Node::variable('b')),
        Node::variable('c'),
    ));
    close(glyph(&nested, 'a').size, 10.0);
}

#[test]
fn unbarred_and_linear_fractions_keep_their_arguments() {
    let mut no_bar = Frac::new(Node::variable('a'), Node::variable('b'));
    no_bar.kind = mt_core::FracKind::NoBar;
    let boxes = lay(Node::Frac(Box::new(no_bar)));
    assert!(
        rules(&boxes).is_empty(),
        "a binomial coefficient draws no rule"
    );
    let numerator = glyph(&boxes, 'a');
    let denominator = glyph(&boxes, 'b');
    // No rule, so the two arguments keep the no-bar gap on both sides of the axis.
    close(
        (denominator.y - 0.45 * 10.0) - (numerator.y + 0.0),
        2.0 * c::FRAC_NOBAR_GAP * 10.0,
    );

    let mut linear = Frac::new(Node::variable('a'), Node::variable('b'));
    linear.kind = mt_core::FracKind::Linear;
    let boxes = lay(Node::Frac(Box::new(linear)));
    close(glyph(&boxes, 'a').size, 10.0);
    close(glyph(&boxes, 'a').y, 0.0);
    assert_eq!(glyph(&boxes, '/').y, 0.0);
    close(
        boxes.width(),
        5.0 + c::FRAC_SLASH_KERN * 10.0 + 5.0 + c::FRAC_SLASH_KERN * 10.0 + 5.0,
    );
}

/// A slash fraction, `a ⁄ b` set with a literal slash instead of a rule.
fn slash_frac_of(numerator: Node, denominator: Node) -> Node {
    let mut frac = Frac::new(numerator, denominator);
    frac.kind = mt_core::FracKind::Slash;
    Node::Frac(Box::new(frac))
}

#[test]
fn slash_fraction_places_the_slash_between_its_arguments() {
    let metrics = ApproxMetrics;
    let width = |glyph: Glyph| metrics.advance(glyph.ch, glyph.font) * glyph.size;
    let kern = c::FRAC_SLASH_KERN * 10.0;

    // At the start of a line the fraction's origin is 0, so the slash must sit one kern after
    // the numerator and the denominator one kern after the slash.
    let boxes = lay(slash_frac_of(Node::variable('a'), Node::variable('b')));
    let (a, slash, b) = (glyph(&boxes, 'a'), glyph(&boxes, '/'), glyph(&boxes, 'b'));
    close(a.x, 0.0);
    close(slash.x, a.x + width(a) + kern);
    close(b.x, slash.x + width(slash) + kern);
    // The stacked-slash shift still applies vertically.
    close(a.y, -c::FRAC_SLASH_SHIFT * 10.0);
    close(slash.y, 0.0);
    close(b.y, c::FRAC_SLASH_SHIFT * 10.0);

    // Nested after a sibling: everything is offset by the fraction's origin, the slash included.
    let boxes = lay(Node::row([
        Node::variable('m'),
        slash_frac_of(Node::variable('a'), Node::variable('b')),
    ]));
    let (m, a, slash, b) = (
        glyph(&boxes, 'm'),
        glyph(&boxes, 'a'),
        glyph(&boxes, '/'),
        glyph(&boxes, 'b'),
    );
    close(m.x, 0.0);
    close(a.x, m.x + width(m));
    close(slash.x, a.x + width(a) + kern);
    close(b.x, slash.x + width(slash) + kern);
}

#[test]
fn fraction_grows_when_the_numerator_carries_limits() {
    let _metrics = ApproxMetrics;
    let plain = lay(frac_of(Node::variable('x'), Node::variable('b')));
    let limits = Limits {
        base: Node::symbol('\u{222B}', Class::Op),
        over: Some(Node::variable('b')),
        under: None,
        align: Side::Center,
    };
    let raised = lay(frac_of(Node::Limits(Box::new(limits)), Node::variable('b')));

    let growth = (raised.height() + raised.depth()) - (plain.height() + plain.depth());
    // The numerator is a script-level integral with a script-script limit above it.
    close(growth, 12.15);
    assert!(growth > 0.0);
    assert!(growth > 0.0);
}

#[test]
fn empty_slots_get_a_dashed_placeholder() {
    let boxes = lay(frac_of(
        Node::empty(),
        Node::row([Node::variable('a'), Node::variable('b')]),
    ));
    let rules = rules(&boxes);
    assert_eq!(rules.len(), 2, "the placeholder and the fraction rule");
    let placeholder = rules
        .iter()
        .find(|rule| rule.dashed)
        .expect("a placeholder");
    let bar = rules
        .iter()
        .find(|rule| !rule.dashed)
        .expect("a fraction rule");
    // The slot keeps the surrounding size, and so does its placeholder.
    close(placeholder.width, c::EMPTY_SLOT_WIDTH * 10.0);
    assert!(placeholder.width > 0.0);
    close(placeholder.height, c::EMPTY_SLOT_HEIGHT * 10.0);
    close(placeholder.depth, c::EMPTY_SLOT_DEPTH * 10.0);
    // The slot's bottom edge keeps the same gap from the rule as a real argument would.
    close(
        placeholder.y + placeholder.depth + c::FRAC_RULE_GAP * 10.0,
        bar.y - bar.depth,
    );
    // The denominator's two glyphs are wider, so they set the width of the fraction.
    close(boxes.width(), 10.0);
    close(bar.width, 10.0);
}

// -------------------------------------------------------------------------------------------
// Scripts
// -------------------------------------------------------------------------------------------

#[test]
fn script_glyphs_use_the_script_scale() {
    let boxes = lay(Node::Scripts(Box::new(Scripts::attach(
        Node::variable('x'),
        Some(Node::variable('i')),
        Some(Node::variable('2')),
    ))));
    close(glyph(&boxes, 'x').size, 10.0);
    close(glyph(&boxes, 'i').size, 0.7 * 10.0);
    close(glyph(&boxes, '2').size, 0.7 * 10.0);
    // Both scripts start the same distance after the base.
    close(glyph(&boxes, 'i').x, glyph(&boxes, '2').x);
    close(glyph(&boxes, 'i').x, 5.0 + c::SCRIPT_KERN * 10.0);
}

#[test]
fn scripts_are_shifted_by_the_base_extents_plus_an_offset() {
    let raised = lay(Node::Scripts(Box::new(Scripts::attach(
        Node::variable('x'),
        None,
        Some(Node::variable('2')),
    ))));
    // (height - depth) / 2 + SCRIPT_OFFSET
    close(
        glyph(&raised, '2').y,
        -((float(0.45) - 0.0) / 2.0 + c::SCRIPT_OFFSET * 10.0),
    );
    close(glyph(&raised, '2').y, -3.75);

    let lowered = lay(Node::Scripts(Box::new(Scripts::attach(
        Node::variable('x'),
        Some(Node::variable('i')),
        None,
    ))));
    // The subscript drops below the base depth by SUBSCRIPT_OFFSET.
    close(glyph(&lowered, 'i').y, 0.0 + c::SUBSCRIPT_OFFSET * 10.0);
    close(lowered.depth(), c::SUBSCRIPT_OFFSET * 10.0 + 0.0);
    close(lowered.height(), float(0.45));
}

#[test]
fn second_nesting_level_uses_the_script_script_scale() {
    // `x` with a subscript, that subscript itself carrying a subscript: `{y_a}` inside `x_{...}`.
    let inner = Node::Scripts(Box::new(Scripts::attach(
        Node::variable('n'),
        Some(Node::variable('a')),
        None,
    )));
    let boxes = lay(Node::Scripts(Box::new(Scripts::attach(
        Node::variable('x'),
        Some(inner),
        None,
    ))));
    close(glyph(&boxes, 'x').size, 10.0);
    close(glyph(&boxes, 'n').size, 0.7 * 10.0);
    close(glyph(&boxes, 'a').size, 0.5 * 10.0);
    // A fraction argument reaches the second level as well.
    let fraction = lay(frac_of(
        Node::variable('x'),
        Node::Scripts(Box::new(Scripts::attach(
            Node::variable('n'),
            Some(Node::variable('a')),
            None,
        ))),
    ));
    close(glyph(&fraction, 'a').size, 0.7 * 10.0); // a script inside the numerator
}

// -------------------------------------------------------------------------------------------
// Inter-atom spacing
// -------------------------------------------------------------------------------------------

#[test]
fn binary_operator_kerns() {
    let plain = lay(Node::row([Node::variable('x'), Node::variable('n')]));
    close(plain.width(), 10.0);
    assert!(kerns(&plain).is_empty(), "ordinary atoms are set solid");

    let sum = lay(Node::row([
        Node::variable('x'),
        Node::symbol('+', Class::Bin),
        Node::variable('n'),
    ]));
    close_all(
        &kerns(&sum),
        &[c::MEDIUM_KERN * 10.0, c::MEDIUM_KERN * 10.0],
    );
    // Besides the operator's own width, the pair of kerns is all that separates the two rows.
    close(
        sum.width() - plain.width() - float(0.78),
        2.0 * c::MEDIUM_KERN * 10.0,
    );

    // A leading binary operator has nothing to combine, so it is an ordinary atom.
    let leading = lay(Node::row([
        Node::symbol('+', Class::Bin),
        Node::variable('x'),
    ]));
    assert!(
        kerns(&leading).is_empty(),
        "no spacing after a leading binary operator"
    );
    close(leading.width(), float(0.78) + 5.0);

    // Relations get the thick space, large operators the thin one.
    let relation = lay(Node::row([
        Node::variable('x'),
        Node::symbol('=', Class::Rel),
        Node::variable('n'),
    ]));
    close_all(
        &kerns(&relation),
        &[c::THICK_KERN * 10.0, c::THICK_KERN * 10.0],
    );
    let operator = lay(Node::row([
        Node::variable('x'),
        Node::symbol('\u{2211}', Class::Op),
    ]));
    close_all(&kerns(&operator), &[c::THIN_KERN * 10.0]);

    // Punctuation and inner atoms separate with a thin space as well.
    let punctuation = lay(Node::row([
        Node::variable('x'),
        Node::symbol(',', Class::Punct),
        Node::variable('n'),
    ]));
    close_all(
        &kerns(&punctuation),
        &[c::THIN_KERN * 10.0, c::THIN_KERN * 10.0],
    );
    let inner = lay(Node::row([
        frac_of(Node::variable('a'), Node::variable('b')),
        Node::variable('n'),
    ]));
    close_all(&kerns(&inner), &[c::THIN_KERN * 10.0]);
}

#[test]
fn a_trailing_binary_operator_is_an_ordinary_atom() {
    let boxes = lay(Node::row([
        Node::variable('x'),
        Node::symbol('+', Class::Bin),
    ]));
    assert!(kerns(&boxes).is_empty());
    close(boxes.width(), 5.0 + float(0.78));
}

#[test]
fn explicit_spaces_add_to_the_automatic_kerns() {
    let automatic = lay(Node::row([
        Node::variable('x'),
        Node::symbol('+', Class::Bin),
        Node::variable('n'),
    ]));
    let explicit = lay(Node::row([
        Node::variable('x'),
        Node::Space(Space::Medium),
        Node::symbol('+', Class::Bin),
        Node::Space(Space::Medium),
        Node::variable('n'),
    ]));
    close(
        explicit.width() - automatic.width(),
        2.0 * c::MEDIUM_KERN * 10.0,
    );

    let quad = lay(Node::Space(Space::Quad));
    close(quad.width(), 10.0);
    let thin = lay(Node::Space(Space::Thin));
    close(thin.width(), c::THIN_KERN * 10.0);
    let back = lay(Node::row([
        Node::variable('x'),
        Node::Space(Space::NegativeThin),
        Node::variable('x'),
    ]));
    close(back.width(), 10.0 - c::THIN_KERN * 10.0);
    close(back.height(), float(0.45));
}

// -------------------------------------------------------------------------------------------
// Delimiters
// -------------------------------------------------------------------------------------------

#[test]
fn fences_stretch_and_kern_towards_the_body() {
    let fenced = lay(Node::Fence(Box::new(Fence {
        open: Delim::Paren,
        close: Delim::Paren,
        body: Node::variable('x'),
    })));
    assert_eq!(glyphs(&fenced).len(), 3);
    let open = glyph(&fenced, '(');
    let body = glyph(&fenced, 'x');
    let closing = glyph(&fenced, ')');
    close(open.x, 0.0);
    close(body.x, float(0.389) - c::FENCE_KERN * 10.0);
    close(closing.x, body.x + 5.0 - c::FENCE_KERN * 10.0);
    close(
        fenced.width(),
        2.0 * float(0.389) + 5.0 - 2.0 * c::FENCE_KERN * 10.0,
    );
    // The delimiter is centred on the body, so it overhangs it on both sides.
    close(open.y, 0.25);
    close(fenced.height(), 7.25);
    close(fenced.depth(), 2.75);
}

#[test]
fn delimiters_absent_from_the_model_take_no_space() {
    let plain = lay(Node::Fence(Box::new(Fence {
        open: Delim::None,
        close: Delim::None,
        body: Node::variable('x'),
    })));
    close(plain.width(), 5.0);
    assert_eq!(glyphs(&plain).len(), 1);

    let one_sided = lay(Node::Fence(Box::new(Fence {
        open: Delim::Brace,
        close: Delim::None,
        body: Node::variable('x'),
    })));
    close(one_sided.width(), float(0.5) - c::FENCE_KERN * 10.0 + 5.0);
    close(glyph(&one_sided, '{').x, 0.0);
    close(glyph(&one_sided, 'x').x, float(0.5) - c::FENCE_KERN * 10.0);
}

#[test]
fn delimiters_grow_with_the_body_and_stretch_vertically_at_the_limit() {
    let tall = Node::row([
        Node::variable('x'),
        Node::variable('n'),
        Node::variable('a'),
    ]);
    let fenced = lay(Node::Fence(Box::new(Fence {
        open: Delim::Paren,
        close: Delim::Paren,
        body: tall,
    })));
    // The body is 15 points wide and only 4.5 tall, so the base-size delimiter already covers it.
    close(glyph(&fenced, '(').size, 10.0);
    close(glyph(&fenced, '(').yscale, 1.0);

    let mut matrix = Matrix::empty(4, 1);
    for row in &mut matrix.rows {
        row[0] = Node::symbol('\u{222B}', Class::Op);
    }
    let huge = lay(Node::Fence(Box::new(Fence {
        open: Delim::Paren,
        close: Delim::Paren,
        body: Node::Matrix(Box::new(matrix)),
    })));
    let delimiter = glyph(&huge, '(');
    close(delimiter.size, c::TALL_GLYPH_SCALES[5] * 10.0);
    assert!(
        delimiter.yscale > 1.0,
        "the largest glyph is stretched vertically"
    );
}

// -------------------------------------------------------------------------------------------
// Matrices
// -------------------------------------------------------------------------------------------

/// A one-column table whose rows are `cells`, with no delimiters and the given alignment.
fn table(cells: Vec<Node>, align: Side) -> Node {
    let mut matrix = Matrix::empty(cells.len(), 1);
    for (index, cell) in cells.into_iter().enumerate() {
        matrix.rows[index][0] = cell;
    }
    matrix.delims = Some((Delim::None, Delim::None));
    matrix.kind = MatrixKind::Array;
    matrix.align = align;
    Node::Matrix(Box::new(matrix))
}

#[test]
fn matrix_column_alignment_moves_cells_inside_the_column() {
    let build = |align| table(vec![Node::text("xxx"), Node::variable('n')], align);
    let left = glyph(&lay(build(Side::Left)), 'n').x;
    let centre = glyph(&lay(build(Side::Center)), 'n').x;
    let right = glyph(&lay(build(Side::Right)), 'n').x;
    // The widest cell of the column is "xxx", 15 points; the narrow one is 5 points wide.
    close(left, 0.0);
    close(centre, 5.0);
    close(right, 10.0);
    assert!(left < centre && centre < right);
}

#[test]
fn matrix_columns_are_separated_and_rows_use_the_math_leading() {
    let mut matrix = Matrix::empty(1, 2);
    matrix.rows[0][0] = Node::variable('x');
    matrix.rows[0][1] = Node::variable('n');
    matrix.delims = Some((Delim::None, Delim::None));
    let boxes = lay(Node::Matrix(Box::new(matrix)));
    close(boxes.width(), 5.0 + c::MATRIX_COL_SEPARATION * 10.0 + 5.0);
    close(glyph(&boxes, 'n').x - glyph(&boxes, 'x').x, 10.0);

    let stacked = lay(table(
        vec![Node::variable('x'), Node::variable('n')],
        Side::Center,
    ));
    let leading = ApproxMetrics.math_leading() * 10.0;
    close(
        glyph(&stacked, 'n').y - glyph(&stacked, 'x').y,
        float(0.45) + leading,
    );
}

#[test]
fn matrices_are_centred_on_the_axis_and_fenced_by_kind() {
    let mut matrix = Matrix::empty(1, 1);
    matrix.rows[0][0] = Node::variable('x');
    let boxes = lay(Node::Matrix(Box::new(matrix)));
    // A single row is shifted up until its centre sits on the axis, so the cell floats above the
    // baseline; the parentheses surround the shifted table.
    close(glyph(&boxes, 'x').y, -0.25);
    // The parentheses are centred on the table's centre, which the shift put on the axis.
    close(boxes.height(), 7.5);
    close(boxes.depth(), 2.5);
    assert_eq!(glyphs(&boxes).len(), 3);

    let mut cases = Matrix::empty(2, 2);
    cases.kind = MatrixKind::Cases;
    cases.rows[0][0] = Node::variable('x');
    let cases = lay(Node::Matrix(Box::new(cases)));
    // `MatrixKind::Cases` draws an opening brace, stretched to the two placeholder rows.
    close(glyph(&cases, '{').size, 2.2 * 10.0);
    assert_eq!(glyphs(&cases).len(), 2, "the brace and the one filled cell");
    assert!(glyphs(&cases).iter().all(|glyph| glyph.ch != '}'));
    assert_eq!(rules(&cases).iter().filter(|rule| rule.dashed).count(), 3);
}

#[test]
fn matrix_rules_are_drawn_when_asked_for() {
    let mut matrix = Matrix::empty(2, 2);
    matrix.rules = true;
    matrix.delims = Some((Delim::None, Delim::None));
    let boxes = lay(Node::Matrix(Box::new(matrix)));
    let rules: Vec<Rule> = rules(&boxes)
        .into_iter()
        .filter(|rule| !rule.dashed)
        .collect();
    assert_eq!(
        rules.len(),
        2,
        "one rule between the rows and one between the columns"
    );
    let horizontal = rules
        .iter()
        .find(|rule| rule.width > rule.height)
        .expect("a row rule");
    let vertical = rules
        .iter()
        .find(|rule| rule.height > rule.width)
        .expect("a column rule");
    close(
        horizontal.height + horizontal.depth,
        c::MATRIX_RULE_THICKNESS * 10.0,
    );
    close(vertical.width, c::MATRIX_RULE_THICKNESS * 10.0);
    close(vertical.y, 0.0);
    assert!(
        vertical.height > vertical.width,
        "the column rule spans the whole table"
    );
}

// -------------------------------------------------------------------------------------------
// Radicals
// -------------------------------------------------------------------------------------------

#[test]
fn radicals_stretch_to_their_radicand() {
    let boxes = lay(Node::Sqrt(Box::new(Sqrt::square(Node::variable('x')))));
    let sign = glyph(&boxes, '\u{221A}');
    close(sign.size, 10.0);
    close(sign.x, 0.0);
    close(glyph(&boxes, 'x').x, float(0.8) + c::RADICAL_KERN * 10.0);
    let bar = rules(&boxes)
        .into_iter()
        .find(|rule| !rule.dashed)
        .expect("an overbar");
    close(bar.width, 5.0 + c::RADICAL_KERN * 10.0);
    close(bar.height + bar.depth, c::RADICAL_RULE_THICKNESS * 10.0);
    // The bar clears the radicand by the radical gap, and the sign reaches the bar's top.
    close(
        bar.y - bar.height,
        -(float(0.45) + c::RADICAL_RULE_GAP * 10.0 + 0.4),
    );
    close(
        boxes.height(),
        float(0.45) + c::RADICAL_RULE_GAP * 10.0 + 0.4,
    );
}

#[test]
fn radicals_grow_a_size_for_a_tall_radicand() {
    let tall = Node::Sqrt(Box::new(Sqrt::square(frac_of(
        Node::variable('a'),
        Node::variable('b'),
    ))));
    let boxes = lay(tall);
    let sign = glyph(&boxes, '\u{221A}');
    // The fraction keeps the surrounding size and is taller, so a bigger sign entry covers it.
    close(sign.size, c::TALL_GLYPH_SCALES[2] * 10.0);
    close(sign.yscale, 1.0);
}

#[test]
fn radical_indices_are_set_small_and_raised() {
    let boxes = lay(Node::Sqrt(Box::new(Sqrt::nth(
        Node::variable('n'),
        Node::variable('x'),
    ))));
    let index = glyph(&boxes, 'n');
    let sign = glyph(&boxes, '\u{221A}');
    close(index.size, 0.7 * 10.0);
    close(index.x, 0.0);
    close(sign.x, index.x + 3.5);
    assert!(
        index.y < sign.y,
        "the index is raised above the radical sign"
    );
    close(boxes.width(), glyph(&boxes, 'x').x + 5.0);
}

// -------------------------------------------------------------------------------------------
// Accents, limits and style overrides
// -------------------------------------------------------------------------------------------

#[test]
fn accents_sit_above_the_base_with_a_gap() {
    let boxes = lay(Node::Accent(Box::new(Accent {
        kind: AccentKind::Hat,
        base: Node::variable('x'),
        under: false,
    })));
    let hat = glyph(&boxes, '\u{02C6}');
    let base = glyph(&boxes, 'x');
    // A single (non-wide) accent keeps its natural size and is centred on the base.
    close(hat.size, 10.0);
    close(hat.y, -(base.y + float(0.45) + c::ACCENT_GAP * 10.0));
    close(
        boxes.height(),
        float(0.45) + c::ACCENT_GAP * 10.0 + 0.25 * 10.0,
    );
    close(boxes.depth(), 0.0);
    // The narrow accent sits inside the base's width.
    close(hat.x, (5.0 - 0.4 * 10.0) / 2.0);
    assert!(hat.x > base.x);
}

#[test]
fn rule_accents_are_drawn_as_rules() {
    let over = lay(Node::Accent(Box::new(Accent {
        kind: AccentKind::Bar,
        base: Node::variable('x'),
        under: false,
    })));
    let rule = rules(&over).into_iter().next().expect("a bar");
    close(rule.width, 5.0);
    close(rule.height + rule.depth, c::ACCENT_RULE_THICKNESS * 10.0);
    close(
        rule.y,
        -(float(0.45) + c::ACCENT_GAP * 10.0 + c::ACCENT_RULE_THICKNESS * 10.0 / 2.0),
    );
    close(
        over.height(),
        float(0.45) + c::ACCENT_GAP * 10.0 + c::ACCENT_RULE_THICKNESS * 10.0,
    );
    close(over.depth(), 0.0);

    let under = lay(Node::Accent(Box::new(Accent {
        kind: AccentKind::Line,
        base: Node::variable('x'),
        under: true,
    })));
    let rule = rules(&under).into_iter().next().expect("an underline");
    close(
        rule.y,
        c::ACCENT_GAP * 10.0 + c::ACCENT_RULE_THICKNESS * 10.0 / 2.0,
    );
    close(under.height(), float(0.45));
    close(
        under.depth(),
        c::ACCENT_GAP * 10.0 + c::ACCENT_RULE_THICKNESS * 10.0,
    );
}

#[test]
fn large_operators_place_limits_above_or_beside() {
    let beside = lay(Node::Big(Box::new(BigOp {
        base: Node::symbol('\u{2211}', Class::Op),
        sub: Some(Node::variable('k')),
        sup: Some(Node::variable('n')),
        limits: false,
    })));
    close(beside.height(), float(0.75) * c::BIG_OP_SCALE_INLINE);
    close(
        beside.depth(),
        float(0.25) * c::BIG_OP_SCALE_INLINE + c::SUBSCRIPT_OFFSET * 10.0,
    );
    close(
        beside.width(),
        10.0 * c::BIG_OP_SCALE_INLINE + c::SCRIPT_KERN * 10.0 + 3.5,
    );
    close(glyph(&beside, 'k').size, 0.7 * 10.0);
    close(
        glyph(&beside, 'k').y,
        float(0.25) * c::BIG_OP_SCALE_INLINE + c::SUBSCRIPT_OFFSET * 10.0,
    );

    let above = lay(Node::Big(Box::new(BigOp {
        base: Node::symbol('\u{2211}', Class::Op),
        sub: Some(Node::variable('k')),
        sup: Some(Node::variable('n')),
        limits: true,
    })));
    close(above.width(), 14.0);
    // The operator is 1.4 times the enclosing size, the limits stay at script size.
    close(glyph(&above, '\u{2211}').size, 14.0);
    close(glyph(&above, 'n').size, 7.0);
    // The operator is shifted down so its centre lands on the axis, which moves the limits with it.
    close(glyph(&above, '\u{2211}').y, 1.0);
    close(glyph(&above, 'n').y, -11.0);
    close(glyph(&above, 'k').y, 9.15);
    close(above.height(), 14.15);
    close(above.depth(), 9.15);
}

#[test]
fn big_operators_grow_more_with_stacked_limits_than_beside() {
    let build = |limits| {
        lay(Node::Big(Box::new(BigOp {
            base: Node::symbol('\u{2211}', Class::Op),
            sub: Some(Node::variable('k')),
            sup: Some(Node::variable('n')),
            limits,
        })))
    };
    let beside = glyph(&build(false), '\u{2211}');
    let above = glyph(&build(true), '\u{2211}');
    close(beside.size, c::BIG_OP_SCALE_INLINE * 10.0);
    close(above.size, c::BIG_OP_SCALE_LIMITS * 10.0);
    // `ApproxMetrics` gives the summation an advance of one em, so its size is its advance.
    assert!(
        above.size > beside.size,
        "stacked limits draw the operator larger: {} vs {}",
        above.size,
        beside.size
    );
    // Only the operator grows; its scripts keep the script size in both constructions.
    for boxes in [build(false), build(true)] {
        close(glyph(&boxes, 'k').size, 0.7 * 10.0);
        close(glyph(&boxes, 'n').size, 0.7 * 10.0);
    }
}

#[test]
fn stacked_limits_share_one_centre_on_the_axis() {
    let boxes = lay(Node::Big(Box::new(BigOp {
        base: Node::symbol('\u{2211}', Class::Op),
        sub: Some(Node::variable('k')),
        sup: Some(Node::variable('n')),
        limits: true,
    })));
    let base = glyph(&boxes, '\u{2211}');
    let over = glyph(&boxes, 'n');
    let under = glyph(&boxes, 'k');
    let metrics = ApproxMetrics;
    // The narrower operator is centred inside the wider limit, so one vertical line runs through
    // the base, the over-script and the under-script.
    let centre = |part: Glyph| part.x + 0.5 * metrics.advance(part.ch, part.font) * part.size;
    close(centre(over), centre(base));
    close(centre(under), centre(base));
    // And the math axis passes through the middle of the operator.
    close(
        base.y - 0.5 * (base.size * 0.75 - base.size * 0.25),
        -float(0.25),
    );
    assert!(
        (centre(base) - 7.0).abs() < 1e-9,
        "the construct is 14 points wide"
    );
}

#[test]
fn integrals_grow_less_than_summations() {
    let integral = |limits| {
        lay(Node::Big(Box::new(BigOp {
            base: Node::symbol('\u{222B}', Class::Op),
            sub: Some(Node::variable('a')),
            sup: Some(Node::variable('b')),
            limits,
        })))
    };
    let summation = lay(Node::Big(Box::new(BigOp {
        base: Node::symbol('\u{2211}', Class::Op),
        sub: Some(Node::variable('a')),
        sup: Some(Node::variable('b')),
        limits: true,
    })));
    close(glyph(&integral(false), '\u{222B}').size, 10.0);
    close(
        glyph(&integral(true), '\u{222B}').size,
        c::BIG_OP_SCALE_INTEGRAL * 10.0,
    );
    // Slimmer than a summation with the same limits, and still larger with limits than without.
    assert!(glyph(&integral(true), '\u{222B}').size < glyph(&summation, '\u{2211}').size);
    assert!(glyph(&integral(true), '\u{222B}').size > glyph(&integral(false), '\u{222B}').size);
    close(glyph(&integral(true), 'a').size, 0.7 * 10.0);
}

#[test]
fn limits_nodes_do_not_enlarge_their_base() {
    // `\lim` is an operator without a display glyph of its own (the model marks it `Op`), and
    // `\overset` is ordinary material: neither grows.
    let cases = lay(Node::Limits(Box::new(Limits {
        base: Node::Styled(Box::new(Styled {
            body: Node::text("lim"),
            style: None,
            font: None,
            class: Some(Class::Op),
        })),
        over: None,
        under: Some(Node::variable('x')),
        align: Side::Center,
    })));
    for part in glyphs(&cases) {
        close(
            part.size,
            if math_base(part.ch) == Some('x') {
                7.0
            } else {
                10.0
            },
        );
    }

    let operator = lay(Node::Limits(Box::new(Limits {
        base: Node::symbol('\u{2211}', Class::Op),
        over: Some(Node::variable('n')),
        under: Some(Node::variable('k')),
        align: Side::Center,
    })));
    close(glyph(&operator, '\u{2211}').size, 10.0);
    close(glyph(&operator, '\u{2211}').y, 0.0);
    close(glyph(&operator, 'n').size, 7.0);
}

#[test]
fn limits_align_inside_the_widest_part() {
    let boxes = lay(Node::Limits(Box::new(Limits {
        base: Node::variable('x'),
        over: Some(Node::text("abc")),
        under: None,
        align: Side::Left,
    })));
    // The over-script is script size, so "abc" is 10.5 points wide against the base's 5.
    close(boxes.width(), 10.5);
    close(glyph(&boxes, 'x').x, 0.0);
    close(glyph(&boxes, 'a').x, 0.0);
    let boxes = lay(Node::Limits(Box::new(Limits {
        base: Node::variable('x'),
        over: Some(Node::text("abc")),
        under: None,
        align: Side::Center,
    })));
    close(glyph(&boxes, 'x').x, (10.5 - 5.0) / 2.0);
    close(glyph(&boxes, 'a').x, 0.0);
    close(
        boxes.height(),
        float(0.45) + c::LIMIT_GAP * 10.0 + 0.45 * 7.0,
    );
}

#[test]
fn styled_overrides_style_font_and_class() {
    let boxes = lay(Node::Styled(Box::new(Styled {
        body: Node::variable('x'),
        style: Some(MathStyle::Script),
        font: Some(Font::Bold),
        class: None,
    })));
    close(glyph(&boxes, 'x').size, 0.7 * 10.0);
    assert_eq!(glyph(&boxes, 'x').font, Font::Bold);

    let inner = || frac_of(Node::variable('a'), Node::variable('b'));
    let plain = lay(Node::row([inner(), Node::variable('n')]));
    let overridden = lay(Node::row([
        Node::Styled(Box::new(Styled {
            body: inner(),
            style: None,
            font: None,
            class: Some(Class::Ord),
        })),
        Node::variable('n'),
    ]));
    close(plain.width() - overridden.width(), c::THIN_KERN * 10.0);
    close(glyph(&plain, 'a').size, 10.0);
}

#[test]
fn error_nodes_are_previewed_as_text() {
    let boxes = lay(Node::Error("?".to_owned()));
    assert_eq!(glyphs(&boxes).len(), 1);
    close(boxes.width(), 5.0);
    close(lay(Node::Align).width(), 0.0);
}
