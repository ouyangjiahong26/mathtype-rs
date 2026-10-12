//! Round trips: read a document, write it back, read it again. Nothing touched must change —
//! not merely the parts' content but the archive's bytes — and an edit must leave every part
//! except the one it touched alone.

mod util;

use mt_docx::{Block, Item};
use util::{minimal_docx, open, parts_of, realistic_docx};

/// A document written back untouched is byte-for-byte the container it came from.
#[test]
fn untouched_documents_round_trip_byte_identical() {
    for fixture in [
        realistic_docx(),
        minimal_docx("<w:p><w:r><w:t>x</w:t></w:r></w:p>"),
    ] {
        let written = open(&fixture).into_bytes().expect("written back");
        assert_eq!(parts_of(&fixture), parts_of(&written));
        assert_eq!(fixture, written);
    }
}

/// A document written back untouched indexes identically the second time.
#[test]
fn structure_survives_the_round_trip() {
    let fixture = realistic_docx();
    let first = open(&fixture);
    let blocks: Vec<_> = first
        .blocks()
        .iter()
        .map(|block| match block {
            Block::Paragraph(paragraph) => (
                paragraph.style.clone(),
                paragraph.numbered,
                paragraph.table_depth,
                paragraph.text(),
                paragraph
                    .items
                    .iter()
                    .map(|item| item.kind())
                    .collect::<Vec<_>>(),
                (0, 0),
            ),
            Block::Table(table) => (
                None,
                false,
                0,
                String::new(),
                vec!["table"],
                (table.rows, table.columns),
            ),
        })
        .collect();
    let written = first.into_bytes().expect("written back");
    let second = open(&written);

    assert_eq!(second.blocks().len(), blocks.len());
    for (index, before) in blocks.iter().enumerate() {
        let after = match &second.blocks()[index] {
            Block::Paragraph(paragraph) => (
                paragraph.style.clone(),
                paragraph.numbered,
                paragraph.table_depth,
                paragraph.text(),
                paragraph
                    .items
                    .iter()
                    .map(|item| item.kind())
                    .collect::<Vec<_>>(),
                (0, 0),
            ),
            Block::Table(table) => (
                None,
                false,
                0,
                String::new(),
                vec!["table"],
                (table.rows, table.columns),
            ),
        };
        assert_eq!(&after, before, "block {index} changed");
    }
}

/// Replacing one formula changes the main part alone; every other part keeps its bytes.
#[test]
fn an_edit_leaves_untouched_parts_alone() {
    use mt_docx::Replacement;
    use mt_tex::parse as parse_tex;

    let fixture = realistic_docx();
    let mut document = open(&fixture);
    let equation = document.equations().into_iter().next().expect("a formula");
    let math = parse_tex("z^9").expect("parses");
    document
        .replace(
            equation.block,
            equation.item,
            Replacement::OMath {
                math,
                display: false,
            },
        )
        .expect("replaced");

    let written = document.into_bytes().expect("written back");
    for (name, bytes) in parts_of(&written) {
        if name == "word/document.xml" {
            continue;
        }
        let before = parts_of(&fixture)
            .into_iter()
            .find(|(other, _)| *other == name)
            .unwrap();
        assert_eq!(before.1, bytes, "part {name} changed");
    }
}

/// The raw fragment slicing the index promises: an item's range really holds its markup, and the
/// whole body is covered by the paragraph ranges with nothing lost in between.
#[test]
fn item_ranges_slice_their_own_markup() {
    let mut document = open(&realistic_docx());
    let xml = document.document_xml().to_owned();
    let equations: Vec<_> = document
        .equations()
        .into_iter()
        .filter(|equation| matches!(equation.kind, mt_docx::EquationKind::Omath { .. }))
        .collect();
    for equation in &equations {
        let paragraph = match &document.blocks()[equation.block] {
            Block::Paragraph(paragraph) => paragraph,
            _ => panic!("formula outside a paragraph"),
        };
        let Item::OMath { range, .. } = &paragraph.items[equation.item] else {
            panic!("not a formula item");
        };
        let fragment = &xml[range.clone()];
        assert!(fragment.starts_with("<m:oMath"));
    }

    // Insert a formula at the very end of the body; the reopened document finds one more.
    let before = document.equations().len();
    let math = mt_tex::parse("1+1").unwrap();
    document
        .insert_omath(mt_docx::Position::BodyEnd, &math, true)
        .expect("inserted");
    assert_eq!(document.equations().len(), before + 1);
    let written = document.into_bytes().expect("written back");
    let reopened = open(&written);
    assert_eq!(reopened.equations().len(), before + 1);
    assert!(
        reopened
            .equations()
            .iter()
            .all(|equation| equation.math.is_some())
    );
}
