//! Output fixtures: namespace declarations and escaping in what `write` emits.

use mt_core::{Class, Delim, Fence, Font, Frac, FracKind, Math, MathStyle, Node, Scripts, TextRun};
use mt_mathml::{parse, write};

/// An expression using most of the tree.
fn sample() -> Math {
    Math::new(Node::row([
        Node::variable('x'),
        Node::symbol('=', Class::Rel),
        Node::Frac(Box::new(Frac { num: Node::text("a < b"), den: Node::variable('c'), kind: FracKind::Bar })),
        Node::Scripts(Box::new(Scripts { base: Node::variable('y'), sub: Some(Node::text("1 & 2")), sup: None })),
        Node::Fence(Box::new(Fence { open: Delim::Paren, close: Delim::Paren, body: Node::variable('z') })),
    ]))
}

#[test]
fn the_document_declares_the_mathml_namespace() {
    let document = write(&sample());
    assert!(document.starts_with("<m:math xmlns:m=\"http://www.w3.org/1998/Math/MathML\""), "{document}");
    assert!(document.contains("xmlns:m=\"http://www.w3.org/1998/Math/MathML\""), "{document}");
    assert!(document.ends_with("</m:math>"), "{document}");
    assert!(document.contains("<m:mi>x</m:mi>"), "{document}");
    // The namespace is declared once, on the root, not on every element.
    assert_eq!(document.matches("xmlns:m=").count(), 1, "{document}");
}

#[test]
fn the_display_attribute_follows_the_equation_style() {
    let block = write(&Math { body: Node::variable('x'), style: MathStyle::Full });
    assert!(block.contains("display=\"block\""), "{block}");
    let inline = write(&Math { body: Node::variable('x'), style: MathStyle::Text });
    assert!(inline.contains("display=\"inline\""), "{inline}");
    let script = write(&Math { body: Node::variable('x'), style: MathStyle::Script });
    assert!(script.contains("display=\"inline\""), "{script}");
    assert!(script.contains("<m:mstyle scriptlevel=\"1\">"), "{script}");
}

#[test]
fn text_content_is_escaped() {
    let equation = Math::new(Node::row([Node::text("a < b & c > \"d\" 'e'"), Node::variable('x')]));
    let document = write(&equation);
    for escaped in ["&lt;", "&amp;", "&gt;", "&quot;", "&apos;"] {
        assert!(document.contains(escaped), "missing {escaped} in {document}");
    }
    for raw in ["a < b", "& c", "> \"d\"", "'e'"] {
        assert!(!document.contains(raw), "unescaped {raw} in {document}");
    }
    assert_eq!(parse(&document).unwrap(), equation);
}

#[test]
fn attribute_values_are_escaped() {
    let equation = Math::new(Node::Fence(Box::new(Fence {
        open: Delim::Custom('&'),
        close: Delim::Custom('"'),
        body: Node::variable('x'),
    })));
    let document = write(&equation);
    assert!(document.contains("open=\"&amp;\""), "{document}");
    assert!(document.contains("close=\"&quot;\""), "{document}");
    assert!(!document.contains("open=\"&\""), "{document}");
    assert_eq!(parse(&document).unwrap(), equation);
}

#[test]
fn a_kept_error_node_is_escaped_too() {
    let equation = Math::new(Node::Error("x < y & \"z\"".to_owned()));
    let document = write(&equation);
    assert!(document.contains("<m:merror>x &lt; y &amp; &quot;z&quot;</m:merror>"), "{document}");
    assert_eq!(parse(&document).unwrap(), equation);
}

#[test]
fn every_face_produces_a_mathvariant_or_a_marker() {
    for font in [Font::Upright, Font::Bold, Font::Monospace, Font::Symbol] {
        let document = write(&Math::new(Node::Text(Box::new(TextRun { text: "abc".to_owned(), font }))));
        assert!(document.contains("<m:mtext"), "{document}");
        assert_eq!(parse(&document).unwrap().body, Node::Text(Box::new(TextRun { text: "abc".to_owned(), font })));
    }
}

#[test]
fn writing_is_idempotent() {
    let once = write(&sample());
    let twice = write(&parse(&once).unwrap());
    assert_eq!(once, twice);

    let styled = write(&Math { body: sample().body, style: MathStyle::ScriptScript });
    assert_eq!(styled, write(&parse(&styled).unwrap()));
}
