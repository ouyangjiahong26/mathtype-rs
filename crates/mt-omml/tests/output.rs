//! Output fixtures: namespace declarations and escaping in what the writers emit.

use mt_core::{Char, Class, Delim, Fence, Font, Frac, FracKind, Math, Node, Scripts};
use mt_omml::{parse, write, write_paragraph};

/// An expression using most of the tree.
fn sample() -> Math {
    Math::new(Node::row([
        Node::variable('x'),
        Node::symbol('=', Class::Rel),
        Node::Frac(Box::new(Frac {
            num: Node::text("a < b"),
            den: Node::variable('c'),
            kind: FracKind::Bar,
        })),
        Node::Scripts(Box::new(Scripts {
            base: Node::variable('y'),
            sub: Some(Node::text("1 & 2")),
            sup: None,
        })),
        Node::Fence(Box::new(Fence {
            open: Delim::Paren,
            close: Delim::Paren,
            body: Node::variable('z'),
        })),
    ]))
}

#[test]
fn the_document_declares_the_office_math_namespace() {
    let document = write(&sample());
    assert!(
        document.starts_with(
            "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">"
        ),
        "{document}"
    );
    assert!(document.ends_with("</m:oMath>"), "{document}");
    assert!(document.contains("<m:r><m:t>x</m:t></m:r>"), "{document}");
    // The namespace is declared once, on the root, not on every element.
    assert_eq!(document.matches("xmlns:m=").count(), 1, "{document}");
    // Every element is written in the `m` namespace, so a Word document can pasted it as is.
    for element in ["<m:f>", "<m:sSub>", "<m:d>", "<m:num>", "<m:den>", "<m:e>"] {
        assert!(
            document.contains(element),
            "missing {element} in {document}"
        );
    }
}

#[test]
fn a_paragraph_declares_the_namespace_and_its_justification() {
    let document = write_paragraph(&sample(), Some("center"));
    assert!(
        document.starts_with(
            "<m:oMathPara xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">"
        ),
        "{document}"
    );
    assert!(
        document.contains("<m:oMathParaPr><m:jc m:val=\"center\"/></m:oMathParaPr>"),
        "{document}"
    );
    assert!(document.contains("<m:oMath>"), "{document}");
    assert!(document.ends_with("</m:oMath></m:oMathPara>"), "{document}");
    assert_eq!(parse(&document).unwrap().body, sample().body);

    let bare = write_paragraph(&sample(), None);
    assert!(!bare.contains("oMathParaPr"), "{bare}");
    assert!(!bare.contains("m:jc"), "{bare}");

    let left = write_paragraph(&sample(), Some("left"));
    assert!(left.contains("m:jc m:val=\"left\""), "{left}");
    // An unknown justification is written as centred rather than passed on.
    let fallback = write_paragraph(&sample(), Some("diagonal"));
    assert!(fallback.contains("m:jc m:val=\"center\""), "{fallback}");
}

#[test]
fn text_content_is_escaped() {
    let equation = Math::new(Node::row([
        Node::text("a < b & c > \"d\" 'e'"),
        Node::variable('x'),
    ]));
    let document = write(&equation);
    for escaped in ["&lt;", "&amp;", "&gt;", "&quot;", "&apos;"] {
        assert!(
            document.contains(escaped),
            "missing {escaped} in {document}"
        );
    }
    for raw in ["a < b", "& c", "> \"d\"", "'e'"] {
        assert!(!document.contains(raw), "unescaped {raw} in {document}");
    }
    assert_eq!(parse(&document).unwrap().body, equation.body);
}

#[test]
fn attribute_values_are_escaped() {
    let equation = Math::new(Node::Fence(Box::new(Fence {
        open: Delim::Custom('&'),
        close: Delim::Custom('"'),
        body: Node::variable('x'),
    })));
    let document = write(&equation);
    assert!(
        document.contains("<m:begChr m:val=\"&amp;\"/>"),
        "{document}"
    );
    assert!(
        document.contains("<m:endChr m:val=\"&quot;\"/>"),
        "{document}"
    );
    assert_eq!(parse(&document).unwrap().body, equation.body);
}

#[test]
fn text_that_needs_its_whitespace_asks_for_it() {
    let padded = write(&Math::new(Node::text(" a ")));
    assert!(
        padded.contains("<m:t xml:space=\"preserve\"> a </m:t>"),
        "{padded}"
    );
    let plain = write(&Math::new(Node::text("a")));
    assert!(plain.contains("<m:t>a</m:t>"), "{plain}");
    assert!(!plain.contains("xml:space"), "{plain}");
}

#[test]
fn a_kept_markup_node_is_written_as_escaped_text() {
    let document = write(&Math::new(Node::Error("x < y & \"z\"".to_owned())));
    assert!(document.contains("<m:nor m:val=\"1\"/>"), "{document}");
    assert!(
        document.contains("x &lt; y &amp; &quot;z&quot;"),
        "{document}"
    );
}

#[test]
fn every_face_produces_run_properties() {
    // A variable is italic by default, so it needs no properties at all.
    assert!(write(&Math::new(Node::variable('x'))).contains("<m:r><m:t>x</m:t></m:r>"));
    for (font, expected) in [
        (Font::Upright, "<m:rPr><m:sty m:val=\"p\"/></m:rPr>"),
        (Font::Bold, "<m:rPr><m:sty m:val=\"b\"/></m:rPr>"),
        (
            Font::Monospace,
            "<m:rPr><m:scr m:val=\"monospace\"/><m:sty m:val=\"p\"/></m:rPr>",
        ),
    ] {
        let document = write(&Math::new(Node::Char(Char {
            ch: 'R',
            class: Class::Ord,
            font,
        })));
        assert!(document.contains(expected), "{font:?} in {document}");
    }
    // Literal text is marked as normal text so that it does not read back as a variable.
    let document = write(&Math::new(Node::text("abc")));
    assert!(
        document.contains("<m:rPr><m:nor m:val=\"1\"/><m:sty m:val=\"p\"/></m:rPr>"),
        "{document}"
    );
    assert!(
        write(&Math::new(Node::text("abc"))).contains("<m:t>abc</m:t>"),
        "{document}"
    );
}

#[test]
fn writing_is_idempotent() {
    let once = write(&sample());
    assert_eq!(once, write(&parse(&once).unwrap()));
    let paragraph = write_paragraph(&sample(), Some("right"));
    let read = parse(&paragraph).unwrap();
    assert_eq!(write(&read), write(&parse(&write(&read)).unwrap()));
}

#[test]
fn control_characters_are_dropped_not_written_raw() {
    // `U+0001`..`U+0008` are illegal in XML 1.0; tab, line feed and carriage return are kept.
    let equation = Math::new(Node::row([Node::text("a\u{0001}b\u{0007}c\td\ne\rf")]));
    let document = write(&equation);
    for forbidden in ['\u{0001}', '\u{0007}'] {
        assert!(
            !document.contains(forbidden),
            "illegal character {forbidden:?} written raw in {document}"
        );
    }
    // Tab and line feed survive raw; carriage return must be escaped (`&#13;`) because XML
    // parsers normalize a raw CR to LF, which would silently lose the character.
    assert!(document.contains('\t'), "tab dropped in {document}");
    assert!(document.contains('\n'), "line feed dropped in {document}");
    assert!(
        document.contains("&#13;"),
        "carriage return neither escaped nor kept in {document}"
    );
    // What the writer emits is well-formed enough for the reader to take back, whitespace
    // intact (`parse` unwraps a single-run body).
    let read = parse(&document).unwrap();
    assert_eq!(read.body, Node::text("abc\td\ne\rf"));
}
