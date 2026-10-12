//! Locating, replacing, and inserting: the editing API against the realistic fixture.

mod util;

use mt_docx::{
    Block, Document, EquationKind, ImageData, Item, Position, Replacement, RunStyle, VertAlign,
};
use mt_tex::{parse as parse_tex, to_latex};
use util::{minimal_docx, open, parts_of, realistic_docx};

/// Kinds of the items of a block, for asserting sequences.
fn kinds(document: &Document, block: usize) -> Vec<&'static str> {
    match &document.blocks()[block] {
        Block::Paragraph(paragraph) => paragraph.items.iter().map(|item| item.kind()).collect(),
        Block::Table(_) => vec!["table"],
    }
}

/// The LaTeX both sides are compared through, so spelling never matters.
fn same_tree(one: &mt_core::Math, other: &mt_core::Math) -> bool {
    to_latex(one) == to_latex(other)
}

/// The index reads the fixture's structure: styles, list flags, table size, item kinds.
#[test]
fn the_index_reads_the_document() {
    let document = open(&realistic_docx());
    let blocks = document.blocks();
    // heading, text+formula, display formula, table, its four cell paragraphs, list, object,
    // plain.
    assert_eq!(blocks.len(), 11);
    let heading = match &blocks[0] {
        Block::Paragraph(paragraph) => paragraph,
        _ => panic!("block 0 is not a paragraph"),
    };
    assert_eq!(heading.style.as_deref(), Some("Heading1"));
    assert_eq!(heading.text(), "A heading");

    let second = match &blocks[1] {
        Block::Paragraph(paragraph) => paragraph,
        _ => panic!("block 1 is not a paragraph"),
    };
    assert_eq!(
        kinds(&document, 1),
        vec!["text", "other", "omath", "other", "text"]
    );
    match &second.items[0] {
        Item::Text { text, style, .. } => {
            assert_eq!(text, "bold text & more ");
            assert_eq!(style.bold, Some(true));
            assert_eq!(style.size_half_points, Some(28));
        }
        _ => panic!("first item is not text"),
    }

    let table = match &blocks[3] {
        Block::Table(table) => table,
        _ => panic!("block 3 is not a table"),
    };
    assert_eq!((table.rows, table.columns), (2, 2));
    // Cell paragraphs follow their table, one depth in.
    for index in 4..8 {
        match &blocks[index] {
            Block::Paragraph(paragraph) => assert_eq!(paragraph.table_depth, 1),
            _ => panic!("block {index} is not a cell paragraph"),
        }
    }
    // The superscript run of the list item.
    let list = match &blocks[8] {
        Block::Paragraph(paragraph) => paragraph,
        _ => panic!("block 8 is not the list paragraph"),
    };
    assert!(list.numbered);
    match &list.items[2] {
        Item::Text { style, .. } => assert_eq!(style.vert_align, Some(VertAlign::Superscript)),
        _ => panic!("third item is not the superscript run"),
    }
    // The picture run.
    match &blocks[7] {
        Block::Paragraph(paragraph) => match &paragraph.items[0] {
            Item::Image { rel_id, name, .. } => {
                assert_eq!(rel_id.as_deref(), Some("rId4"));
                assert_eq!(name.as_deref(), Some("fixture picture"));
            }
            _ => panic!("first item of the picture cell is not an image"),
        },
        _ => panic!("block 7 is not the picture cell"),
    }
}

/// Every formula is found — inline, display, in a table cell, as an object — with its tree.
#[test]
fn every_formula_is_located_with_its_tree() {
    let document = open(&realistic_docx());
    let equations = document.equations();
    assert_eq!(equations.len(), 5, "three OMML and two OLE formulas");
    assert!(same_tree(
        equations[0].math.as_ref().unwrap(),
        &parse_tex("a^2+b_2").unwrap()
    ));
    assert!(same_tree(
        equations[1].math.as_ref().unwrap(),
        &parse_tex("\\int_0^1 x\\,dx").unwrap()
    ));
    assert!(matches!(
        equations[1].kind,
        EquationKind::Omath { display: true }
    ));
    assert!(same_tree(
        equations[2].math.as_ref().unwrap(),
        &parse_tex("\\sqrt{2}").unwrap()
    ));
    for equation in &equations[3..] {
        let EquationKind::Ole { prog_id, part } = &equation.kind else {
            panic!("expected an object formula");
        };
        assert_eq!(prog_id.as_deref(), Some("Equation.3"));
        assert_eq!(part.as_deref(), Some("word/embeddings/oleObject1.bin"));
        assert!(same_tree(
            equation.math.as_ref().unwrap(),
            &parse_tex("\\frac{a}{b}").unwrap()
        ));
    }
}

/// Replacing an Office Math formula with another one: tree in, tree out, position kept.
#[test]
fn an_omath_formula_is_replaced_by_a_tree() {
    let mut document = open(&realistic_docx());
    let equation = document.equations().into_iter().next().expect("a formula");
    let replacement = parse_tex("\\frac{p}{q}+r").unwrap();
    document
        .replace(
            equation.block,
            equation.item,
            Replacement::OMath {
                math: replacement,
                display: false,
            },
        )
        .expect("replaced");

    // The neighbouring text runs are untouched; the slot still holds a formula.
    assert_eq!(
        kinds(&document, equation.block),
        vec!["text", "other", "omath", "other", "text"]
    );
    let after = document
        .equations()
        .into_iter()
        .next()
        .expect("still a formula");
    assert!(same_tree(
        after.math.as_ref().unwrap(),
        &parse_tex("\\frac{p}{q}+r").unwrap()
    ));

    // And it survives a write and a reopen.
    let written = document.into_bytes().expect("written");
    let reopened = open(&written);
    let again = reopened
        .equations()
        .into_iter()
        .next()
        .expect("still a formula");
    assert!(same_tree(
        again.math.as_ref().unwrap(),
        &parse_tex("\\frac{p}{q}+r").unwrap()
    ));
}

/// Replacing a formula with text: a styled run takes the formula's place.
#[test]
fn a_formula_is_replaced_by_text() {
    let mut document = open(&realistic_docx());
    let equation = document.equations().into_iter().next().expect("a formula");
    document
        .replace(
            equation.block,
            equation.item,
            Replacement::Text {
                text: "x < y".to_owned(),
                style: RunStyle {
                    bold: Some(true),
                    italic: Some(false),
                    ..RunStyle::default()
                },
            },
        )
        .expect("replaced");
    assert_eq!(
        kinds(&document, equation.block),
        vec!["text", "other", "text", "other", "text"]
    );
    match &document.blocks()[equation.block] {
        Block::Paragraph(paragraph) => match &paragraph.items[2] {
            Item::Text { text, style, .. } => {
                assert_eq!(text, "x < y");
                assert_eq!(style.bold, Some(true));
                assert_eq!(style.italic, Some(false));
            }
            _ => panic!("the slot is not text"),
        },
        _ => panic!("not a paragraph"),
    }
    let written = document.into_bytes().expect("written");
    let document_xml = String::from_utf8_lossy(
        &parts_of(&written)
            .into_iter()
            .find(|(name, _)| name == "word/document.xml")
            .unwrap()
            .1,
    )
    .into_owned();
    assert!(document_xml.contains("x &lt; y"));
}

/// Replacing a formula with a picture: a media part and a relationship appear.
#[test]
fn a_formula_is_replaced_by_an_image() {
    let mut document = open(&realistic_docx());
    let equation = document.equations().into_iter().next().expect("a formula");
    let before = document.part_names().count();
    document
        .replace(
            equation.block,
            equation.item,
            Replacement::Image(ImageData {
                bytes: util::PNG.to_vec(),
                extension: "png".to_owned(),
                content_type: "image/png".to_owned(),
                name: "replacement".to_owned(),
                extent_emu: (9525, 9525),
            }),
        )
        .expect("replaced");
    assert_eq!(
        kinds(&document, equation.block),
        vec!["text", "other", "image", "other", "text"]
    );
    let written = document.into_bytes().expect("written");
    let parts = parts_of(&written);
    let media = parts
        .iter()
        .find(|(name, _)| name == "word/media/image2.png");
    assert!(media.is_some(), "a new media part appears");
    assert_eq!(media.unwrap().1, util::PNG);
    assert_eq!(parts.len(), before + 1, "exactly one part was added");
    let rels = String::from_utf8_lossy(
        &parts
            .iter()
            .find(|(name, _)| name == "word/_rels/document.xml.rels")
            .unwrap()
            .1,
    )
    .into_owned();
    assert!(rels.contains("Target=\"media/image2.png\""));
}

/// Replacing a formula with an object: a fresh compound file, a bin default, a relationship.
#[test]
fn a_formula_is_replaced_by_an_ole_object() {
    let mut document = open(&realistic_docx());
    let equation = document.equations().into_iter().next().expect("a formula");
    let math = parse_tex("\\sum_i i^2").unwrap();
    document
        .replace(
            equation.block,
            equation.item,
            Replacement::Ole {
                math,
                prog_id: None,
                preview: None,
            },
        )
        .expect("replaced");
    assert_eq!(
        kinds(&document, equation.block),
        vec!["text", "other", "object", "other", "text"]
    );

    let written = document.into_bytes().expect("written");
    let parts = parts_of(&written);
    assert!(
        parts
            .iter()
            .any(|(name, _)| name == "word/embeddings/oleObject2.bin"),
        "a new embedding part appears"
    );

    let reopened = open(&written);
    let after = reopened
        .equations()
        .into_iter()
        .next()
        .expect("still a formula");
    let EquationKind::Ole { part, .. } = &after.kind else {
        panic!("the slot now holds an object");
    };
    assert_eq!(part.as_deref(), Some("word/embeddings/oleObject2.bin"));
    assert!(same_tree(
        after.math.as_ref().unwrap(),
        &parse_tex("\\sum_i i^2").unwrap()
    ));
}

/// Rewriting an object's equation: the stream inside the existing compound file changes, the part
/// keeps its name, and the preview markup survives.
#[test]
fn an_ole_equation_is_rewritten_in_place() {
    let mut document = open(&realistic_docx());
    let equations = document.equations();
    // The object in the table cell, which carries a preview picture.
    let object = equations
        .iter()
        .find(|equation| matches!(equation.kind, EquationKind::Ole { .. }))
        .expect("an object formula");
    let math = parse_tex("\\sqrt[n]{x}").unwrap();
    document
        .replace(
            object.block,
            object.item,
            Replacement::Ole {
                math,
                prog_id: None,
                preview: None,
            },
        )
        .expect("rewritten");

    // No new part: the existing one was rewritten.
    assert_eq!(
        document
            .part_names()
            .filter(|name| name.contains("embeddings"))
            .count(),
        1
    );
    let written = document.into_bytes().expect("written");
    let parts = parts_of(&written);
    assert!(
        parts
            .iter()
            .any(|(name, _)| name == "word/embeddings/oleObject1.bin")
    );
    // The preview markup is still there, pointing at the same picture.
    let document_xml = String::from_utf8_lossy(
        &parts
            .iter()
            .find(|(name, _)| name == "word/document.xml")
            .unwrap()
            .1,
    )
    .into_owned();
    assert!(document_xml.contains("v:shapetype"));
    assert!(document_xml.contains("r:id=\"rId4\""));

    let reopened = open(&written);
    let after = reopened
        .equations()
        .into_iter()
        .find(|equation| matches!(equation.kind, EquationKind::Ole { .. }))
        .expect("an object formula");
    assert!(same_tree(
        after.math.as_ref().unwrap(),
        &parse_tex("\\sqrt[n]{x}").unwrap()
    ));
}

/// Replacing every object with a formula retires the objects' relationship once nothing uses it,
/// but leaves the part in the container for whoever else may point at it.
#[test]
fn ole_formulas_are_replaced_by_omath() {
    let mut document = open(&realistic_docx());
    loop {
        let Some(object) = document
            .equations()
            .into_iter()
            .find(|equation| matches!(equation.kind, EquationKind::Ole { .. }))
        else {
            break;
        };
        let math = parse_tex("e^{i\\pi}").unwrap();
        document
            .replace(
                object.block,
                object.item,
                Replacement::OMath {
                    math,
                    display: false,
                },
            )
            .expect("replaced");
    }
    assert!(
        document
            .equations()
            .iter()
            .all(|equation| matches!(equation.kind, EquationKind::Omath { .. }))
    );

    let written = document.into_bytes().expect("written");
    let parts = parts_of(&written);
    let rels = String::from_utf8_lossy(
        &parts
            .iter()
            .find(|(name, _)| name == "word/_rels/document.xml.rels")
            .unwrap()
            .1,
    )
    .into_owned();
    assert!(
        !rels.contains("oleObject1.bin"),
        "the object's relationship is retired"
    );
    assert!(
        parts
            .iter()
            .any(|(name, _)| name == "word/embeddings/oleObject1.bin"),
        "the part itself stays"
    );
    let reopened = open(&written);
    assert!(
        reopened
            .equations()
            .iter()
            .all(|equation| matches!(equation.kind, EquationKind::Omath { .. }))
    );
}

/// Insertion: a formula after a display formula, a field at a paragraph's end, raw XML before an
/// item, and a new paragraph at the body's end.
#[test]
fn formulas_and_fields_are_inserted() {
    let mut document = open(&minimal_docx(
        "<w:p><m:oMathPara><m:oMath><m:r><m:t>E</m:t></m:r></m:oMath></m:oMathPara></w:p>\
         <w:p><w:r><w:t>tail</w:t></w:r></w:p>",
    ));
    assert_eq!(document.equations().len(), 1);

    // A field right after the display formula, raw XML as the caller sees fit.
    let field = "<w:r xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
                 <w:fldChar w:fldCharType=\"begin\"/></w:r>";
    document
        .insert_field(Position::AfterItem { block: 0, item: 0 }, field)
        .expect("field inserted");
    assert_eq!(kinds(&document, 0), vec!["omath", "other"]);
    assert!(document.document_xml().contains("fldCharType=\"begin\""));

    // An inline formula at the end of the second paragraph.
    let math = parse_tex("a+b").unwrap();
    document
        .insert_omath(Position::ParagraphEnd { block: 1 }, &math, false)
        .expect("inserted");
    assert_eq!(kinds(&document, 1), vec!["text", "omath"]);
    assert_eq!(document.equations().len(), 2);

    // A new display paragraph at the end of the body.
    let math = parse_tex("c").unwrap();
    document
        .insert_omath(Position::BodyEnd, &math, true)
        .expect("inserted");
    assert_eq!(document.blocks().len(), 3);
    assert_eq!(document.equations().len(), 3);

    // Raw XML before an item lands exactly there and folds into the index as other content.
    document
        .insert_fragment(
            Position::BeforeItem { block: 1, item: 0 },
            "<w:bookmarkStart w:id=\"9\" w:name=\"x\"/>",
        )
        .expect("inserted");
    assert!(document.document_xml().contains("w:name=\"x\""));
    let written = document.into_bytes().expect("written");
    let reopened = open(&written);
    assert_eq!(reopened.equations().len(), 3);
    assert_eq!(reopened.blocks().len(), 3);
}

/// Batch conversion shape: coordinates collected up front, edited back to front so earlier ones
/// stay valid — exactly the document-level converter's shape.
#[test]
fn a_whole_document_of_formulas_is_rewritten_in_turn() {
    let mut document = open(&realistic_docx());
    let targets: Vec<_> = document
        .equations()
        .into_iter()
        .filter(|equation| matches!(equation.kind, EquationKind::Omath { .. }))
        .map(|equation| {
            (
                equation.block,
                equation.item,
                equation.math.expect("a tree"),
            )
        })
        .collect();
    assert_eq!(targets.len(), 3);
    for (block, item, math) in targets.into_iter().rev() {
        // Whatever the tree says, square it.
        let rewritten = parse_tex(&format!("({})^2", to_latex(&math))).expect("parses");
        document
            .replace(
                block,
                item,
                Replacement::OMath {
                    math: rewritten,
                    display: false,
                },
            )
            .expect("replaced");
    }
    let equations = document.equations();
    assert_eq!(equations.len(), 5);
    assert!(equations.iter().all(|equation| equation.math.is_some()));
    assert!(
        equations
            .iter()
            .filter(|equation| matches!(equation.kind, EquationKind::Omath { .. }))
            .all(|equation| to_latex(equation.math.as_ref().unwrap()).contains("^2"))
    );
}

#[test]
fn resizing_an_object_preview_rewrites_shape_and_extent() {
    let mut document = open(&realistic_docx());
    let equations = document.equations();
    let equation = &equations[3]; // the first of the two OLE formulas, whose shape has a preview
    document
        .resize_object_preview(equation.block, equation.item, 90.0, 40.0)
        .expect("the shape resizes");
    let xml = document.document_xml();
    assert!(
        xml.contains("width:90.0pt;height:40.0pt"),
        "the shape style carries the natural size"
    );
    assert!(
        xml.contains("dxaOrig=\"1800\""),
        "dxaOrig is 20 twips per point"
    );
    assert!(
        xml.contains("dyaOrig=\"800\""),
        "dyaOrig is 20 twips per point"
    );
}
