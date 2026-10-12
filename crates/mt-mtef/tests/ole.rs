//! Taking an equation in and out of the compound file around it.

use std::io::{Cursor, Read, Write};

use cfb::CompoundFile;
use mt_core::{Math, Node};
use mt_mtef::{parse, read_from_ole, write, write_into_ole};

/// The equation stream a document embeds.
const EQUATION_STREAM: &str = "ObjectPool/1/Equation Native";
/// Another stream the object holds, which must survive an equation being replaced.
const OTHER_STREAM: &str = "ObjectPool/1/Data";
/// What that other stream holds.
const OTHER_CONTENTS: &[u8] = b"a stream that is not an equation";

fn equation(text: char) -> Math {
    Math::new(Node::row([Node::variable(text), Node::variable('x')]))
}

/// Build a compound file holding an equation, the way a document embeds one.
fn document(math: &Math) -> Vec<u8> {
    let mut file =
        CompoundFile::create(Cursor::new(Vec::new())).expect("a compound file is created");
    file.create_storage("ObjectPool")
        .expect("the object storage is created");
    file.create_storage("ObjectPool/1")
        .expect("the object's own storage is created");
    {
        let mut stream = file
            .create_stream(EQUATION_STREAM)
            .expect("the equation stream is created");
        stream
            .write_all(&write(math))
            .expect("the equation is written");
    }
    {
        let mut stream = file
            .create_stream(OTHER_STREAM)
            .expect("the other stream is created");
        stream
            .write_all(OTHER_CONTENTS)
            .expect("the other stream is written");
    }
    file.flush().expect("the compound file is flushed");
    file.into_inner().into_inner()
}

fn contents(ole: &[u8], path: &str) -> Vec<u8> {
    let mut file = CompoundFile::open(Cursor::new(ole.to_vec())).expect("the compound file opens");
    let mut stream = file.open_stream(path).expect("the stream opens");
    let mut contents = Vec::new();
    stream.read_to_end(&mut contents).expect("the stream reads");
    contents
}

#[test]
fn reads_the_equation_of_a_compound_file() {
    let math = equation('a');
    let ole = document(&math);

    let stream = read_from_ole(&ole)
        .expect("the compound file reads")
        .expect("it holds an equation");
    let body = mt_mtef::Stream::split(&stream)
        .expect("the stream splits")
        .body;
    assert_eq!(parse(&stream).expect("the equation reads"), math);
    assert_eq!(
        body.first(),
        Some(&5),
        "the equation stream starts at the version byte"
    );
}

#[test]
fn replaces_the_equation_of_a_compound_file() {
    let ole = document(&equation('a'));
    let replacement = equation('b');

    let updated = write_into_ole(&ole, &write(&replacement)).expect("the equation is replaced");

    let stream = read_from_ole(&updated)
        .expect("the updated file reads")
        .expect("it holds an equation");
    assert_eq!(parse(&stream).expect("the new equation reads"), replacement);
    assert_eq!(
        contents(&updated, OTHER_STREAM),
        OTHER_CONTENTS,
        "the other stream is untouched"
    );
    assert_eq!(
        contents(&updated, "ObjectPool/1/Equation Native"),
        write(&replacement),
        "the equation stream holds the new equation",
    );

    // Replacing an equation in a file that holds none is an error.
    let mut empty =
        CompoundFile::create(Cursor::new(Vec::new())).expect("a compound file is created");
    empty
        .create_storage("ObjectPool")
        .expect("the storage is created");
    {
        let mut stream = empty
            .create_stream("ObjectPool/Data")
            .expect("a stream is created");
        stream
            .write_all(b"not an equation")
            .expect("the stream is written");
    }
    empty.flush().expect("the compound file is flushed");
    let plain = empty.into_inner().into_inner();

    assert_eq!(
        read_from_ole(&plain).expect("a file without an equation reads"),
        None
    );
    assert!(write_into_ole(&plain, &write(&equation('c'))).is_err());
}
