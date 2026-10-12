//! The smallest XML element tree the reader needs.
//!
//! `quick-xml` hands out events; the converters are much easier to read against a materialised
//! tree, and the documents involved are equations rather than data dumps, so the extra allocation
//! per element buys clarity. Names are reduced to their local part, which is what lets the reader
//! accept MathML with or without the `m:` prefix.

use crate::ParseError;
use quick_xml::Reader;
use quick_xml::escape::{resolve_predefined_entity, unescape};
use quick_xml::events::{BytesStart, Event};

/// The deepest element nesting the reader accepts.
///
/// The converters walk the element tree with recursion, so a document nested deeper than this is
/// rejected with [`ParseError::Nesting`] before the walk starts rather than overflowing the
/// stack. Real equations nest a few dozen levels at most (a table in a fraction in a root, say),
/// so the limit only ever bites on hostile input.
pub(crate) const MAX_DEPTH: usize = 128;

/// One child of an element.
pub(crate) enum Xml {
    /// Nested markup.
    Elem(Elem),
    /// Character data with references already resolved.
    Text(String),
}

/// An element with its attributes and children.
pub(crate) struct Elem {
    /// Local name, without any namespace prefix.
    pub(crate) name: String,
    /// Attributes by local name.
    pub(crate) attrs: Vec<(String, String)>,
    /// Children in document order.
    pub(crate) children: Vec<Xml>,
}

impl Elem {
    /// Attribute value by local name, last one winning.
    pub(crate) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Child elements in document order.
    pub(crate) fn elems(&self) -> impl Iterator<Item = &Elem> {
        self.children.iter().filter_map(|child| match child {
            Xml::Elem(element) => Some(element),
            Xml::Text(_) => None,
        })
    }

    /// All character data directly inside the element, concatenated.
    pub(crate) fn text(&self) -> String {
        let mut text = String::new();
        for child in &self.children {
            if let Xml::Text(part) = child {
                text.push_str(part);
            }
        }
        text
    }

    /// Whether `name` is `"true"` or `"1"`, the spellings Office and MathML accept.
    pub(crate) fn flag(&self, name: &str) -> bool {
        matches!(self.attr(name), Some("true") | Some("1") | Some("on"))
    }
}

/// Reads a whole document and returns its root element.
pub(crate) fn document(xml: &str) -> Result<Elem, ParseError> {
    let mut reader = Reader::from_str(xml);
    {
        let config = reader.config_mut();
        config.check_end_names = true;
        config.expand_empty_elements = false;
        config.trim_text_start = false;
        config.trim_text_end = false;
    }
    let mut open: Vec<Elem> = Vec::new();
    let mut root: Option<Elem> = None;
    loop {
        let event = reader
            .read_event()
            .map_err(|error| ParseError::Xml(error.to_string()))?;
        match event {
            Event::Start(start) => {
                if open.len() >= MAX_DEPTH {
                    return Err(ParseError::Nesting);
                }
                open.push(element(&start)?);
            }
            Event::Empty(start) => attach(&mut open, &mut root, element(&start)?)?,
            Event::End(_) => {
                let closed = open.pop().ok_or_else(|| {
                    ParseError::Xml("a closing tag has no opening tag".to_owned())
                })?;
                attach(&mut open, &mut root, closed)?;
            }
            Event::Text(text) => {
                let normalized = text.xml10_content();
                let decoded =
                    unescape(&normalized).map_err(|error| ParseError::Xml(error.to_string()))?;
                characters(&mut open, decoded.into_owned())?;
            }
            Event::CData(data) => characters(&mut open, data.into_inner().into_owned())?,
            Event::GeneralRef(reference) => {
                characters(&mut open, referenced(&reference.into_inner())?)?
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if let Some(element) = open.last() {
        return Err(ParseError::Xml(format!(
            "element <{}> is never closed",
            element.name
        )));
    }
    root.ok_or_else(|| ParseError::Xml("the document has no root element".to_owned()))
}

/// Turns one start tag into an element.
fn element(start: &BytesStart<'_>) -> Result<Elem, ParseError> {
    let mut attrs = Vec::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|error| ParseError::Xml(error.to_string()))?;
        let value =
            unescape(&attribute.value).map_err(|error| ParseError::Xml(error.to_string()))?;
        attrs.push((
            attribute.key.local_name().into_inner().to_owned(),
            value.into_owned(),
        ));
    }
    Ok(Elem {
        name: start.local_name().into_inner().to_owned(),
        attrs,
        children: Vec::new(),
    })
}

/// Appends a finished element to its parent, or makes it the root.
fn attach(open: &mut Vec<Elem>, root: &mut Option<Elem>, element: Elem) -> Result<(), ParseError> {
    match open.last_mut() {
        Some(parent) => {
            parent.children.push(Xml::Elem(element));
            Ok(())
        }
        None if root.is_none() => {
            *root = Some(element);
            Ok(())
        }
        None => Err(ParseError::Xml(
            "the document has more than one root element".to_owned(),
        )),
    }
}

/// Appends character data to the innermost open element.
fn characters(open: &mut [Elem], text: String) -> Result<(), ParseError> {
    match open.last_mut() {
        Some(parent) if !text.is_empty() => {
            parent.children.push(Xml::Text(text));
            Ok(())
        }
        Some(_) => Ok(()),
        None => {
            if text.trim().is_empty() {
                Ok(())
            } else {
                Err(ParseError::Xml(
                    "character data outside of the root element".to_owned(),
                ))
            }
        }
    }
}

/// Resolves one `&…;` reference to the text it stands for.
fn referenced(reference: &str) -> Result<String, ParseError> {
    if let Some(digits) = reference.strip_prefix('#') {
        let code = match digits.strip_prefix(['x', 'X']) {
            Some(hexadecimal) => u32::from_str_radix(hexadecimal, 16),
            None => digits.parse::<u32>(),
        };
        return code
            .ok()
            .and_then(char::from_u32)
            .map(String::from)
            .ok_or_else(|| ParseError::Xml(format!("&{reference}; is not a character reference")));
    }
    resolve_predefined_entity(reference)
        .map(str::to_owned)
        .ok_or_else(|| ParseError::Xml(format!("&{reference}; is not a known entity")))
}
