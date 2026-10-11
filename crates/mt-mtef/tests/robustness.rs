//! Reading streams that are damaged or cut short: every one of them must come back as a value
//! or an error, never as a panic.

use mt_core::{Class, Delim, Fence, Font, Frac, FracKind, Math, Node, Sqrt};
use mt_mtef::{parse_bare, read_bare, write_bare, MtefError};

/// An equation that uses most of the records the writer emits.
fn rich_equation() -> Math {
    let variable = |name: char| Node::variable(name);
    Math::new(Node::row([
        Node::Frac(Box::new(Frac {
            num: Node::row([variable('a'), Node::symbol('+', Class::Bin), variable('b')]),
            den: Node::Char(mt_core::Char { ch: 'x', class: Class::Ord, font: Font::Bold }),
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
