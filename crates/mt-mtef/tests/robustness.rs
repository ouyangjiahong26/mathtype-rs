//! Reading streams that are damaged or cut short: every one of them must come back as a value
//! or an error, never as a panic.

use mt_core::{Class, Delim, Fence, Font, Frac, FracKind, Math, Node, Sqrt};
use mt_mtef::{MtefError, parse_bare, read_bare, write_bare};

/// An equation that uses most of the records the writer emits.
fn rich_equation() -> Math {
    let variable = |name: char| Node::variable(name);
    Math::new(Node::row([
        Node::Frac(Box::new(Frac {
            num: Node::row([variable('a'), Node::symbol('+', Class::Bin), variable('b')]),
            den: Node::Char(mt_core::Char {
                ch: 'x',
                class: Class::Ord,
                font: Font::Bold,
            }),
            kind: FracKind::Bar,
        })),
        Node::Sqrt(Box::new(Sqrt::nth(variable('n'), variable('x')))),
        Node::Fence(Box::new(Fence {
            open: Delim::Paren,
            close: Delim::Bracket,
            body: Node::row([variable('y'), Node::symbol('=', Class::Rel)]),
        })),
    ]))
}

#[test]
fn cut_short_streams_do_not_panic() {
    let bytes = write_bare(&rich_equation());
    for length in 0..bytes.len() {
        // A cut-short stream is either still readable or an error, and reading it does not panic.
        let _ = read_bare(&bytes[..length]);
    }
    assert!(parse_bare(&bytes).is_ok());
}

#[test]
fn damaged_streams_do_not_panic() {
    let bytes = write_bare(&rich_equation());
    for index in 0..bytes.len() {
        for mask in [0x01, 0x5A, 0x80, 0xFF] {
            let mut damaged = bytes.clone();
            damaged[index] ^= mask;
            // The reader may read anything at all out of a damaged stream; it may not panic.
            let _ = read_bare(&damaged);
        }
    }
    // A stream that is nothing but noise is rejected rather than read as an equation.
    assert!(matches!(read_bare(&[0x00; 64]), Err(MtefError::Version(0))));
    assert_eq!(read_bare(&[]).unwrap_err(), MtefError::Truncated);
}

/// The header of a bare equation stream: version, platform, producer and the equation options.
fn bare_header() -> Vec<u8> {
    vec![5, 1, 0, 0, 0, 0x00, 0x00]
}

#[test]
fn a_ruler_line_nesting_in_itself_is_rejected() {
    // Each `TAG_LINE` with `OPT_LP_RULER` reads another record, which is another line: the
    // recursion never enters an object list, so before the gate this looped until the stack or
    // the input ran out.
    let mut bytes = bare_header();
    for _ in 0..10_000 {
        bytes.extend_from_slice(&[0x01, 0x02]); // TAG_LINE, OPT_LP_RULER
    }
    assert_eq!(read_bare(&bytes).unwrap_err(), MtefError::Nesting);
}

#[test]
fn a_ruler_pile_nesting_in_itself_is_rejected() {
    let mut bytes = bare_header();
    for _ in 0..10_000 {
        // TAG_PILE, OPT_LP_RULER, then the two alignment bytes.
        bytes.extend_from_slice(&[0x04, 0x02, 0x00, 0x00]);
    }
    assert_eq!(read_bare(&bytes).unwrap_err(), MtefError::Nesting);
}

#[test]
fn a_chain_of_scripts_is_bounded() {
    let mut bytes = bare_header();
    for _ in 0..2_000 {
        // TAG_TMPL, no options, selector 27 (scripts), variation 0, template options, END.
        bytes.extend_from_slice(&[0x03, 0x00, 0x1B, 0x00, 0x00, 0x00]);
    }
    bytes.push(0x00); // END the outer list
    assert_eq!(read_bare(&bytes).unwrap_err(), MtefError::Nesting);
}

#[test]
fn a_chain_of_embellishments_is_bounded() {
    let mut bytes = bare_header();
    // TAG_CHAR, OPT_CHAR_EMBELL, typeface 0, a two-byte character code.
    bytes.extend_from_slice(&[0x02, 0x01, 0x80, 0x00, 0x00]);
    for _ in 0..2_000 {
        // TAG_EMBELL, no options, embellishment type 2 (a dot).
        bytes.extend_from_slice(&[0x06, 0x00, 0x02]);
    }
    bytes.push(0x00); // END the embellishment list and the outer list
    assert_eq!(read_bare(&bytes).unwrap_err(), MtefError::Nesting);
}
