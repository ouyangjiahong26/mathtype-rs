//! `[Content_Types].xml`: the content type of every part, by extension default or by override.

use std::collections::BTreeMap;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::{xml_error, DocxError};

/// The namespace of the content types part.
const NAMESPACE: &str = "http://schemas.openxmlformats.org/package/2006/content-types";

/// Content types by default extension and by part-name override.
#[derive(Default)]
pub struct ContentTypes {
    defaults: BTreeMap<String, String>,
    overrides: BTreeMap<String, String>,
}

impl ContentTypes {
    /// Parses the part; an empty or absent part is an empty set of types, not an error.
    pub fn parse(bytes: &[u8]) -> Result<Self, DocxError> {
        if bytes.is_empty() {
            return Ok(Self::default());
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| DocxError::NotUtf8("[Content_Types].xml".to_owned()))?;
        let mut reader = Reader::from_str(text);
        reader.config_mut().trim_text(false);
        let mut types = Self::default();
        loop {
            match reader.read_event().map_err(xml_error)? {
                Event::Start(start) => match start.local_name().as_ref() {
                    "Default" => types.set_default(&start),
                    "Override" => types.set_override(&start),
                    _ => {}
                },
                Event::Empty(start) => match start.local_name().as_ref() {
                    "Default" => types.set_default(&start),
                    "Override" => types.set_override(&start),
                    _ => {}
                },
                Event::Eof => break,
                _ => {}
            }
        }
        Ok(types)
    }

    fn set_default(&mut self, element: &quick_xml::events::BytesStart<'_>) {
        if let (Some(extension), Some(kind)) = (attribute(element, "Extension"), attribute(element, "ContentType")) {
            self.defaults.insert(extension.to_lowercase(), kind);
        }
    }

    fn set_override(&mut self, element: &quick_xml::events::BytesStart<'_>) {
        if let (Some(name), Some(kind)) = (attribute(element, "PartName"), attribute(element, "ContentType")) {
            self.overrides.insert(name, kind);
        }
    }


    /// Ensures the extension default exists, adding it when it does not. Returns whether the set
    /// changed, so the caller knows to write the part back.
    pub fn ensure_default(&mut self, extension: &str, content_type: &str) -> bool {
        let key = extension.to_lowercase();
        match self.defaults.get(&key) {
            Some(existing) if existing == content_type => false,
            _ => {
                self.defaults.insert(key, content_type.to_owned());
                true
            }
        }
    }


    /// Serialises the part, defaults first then overrides, both in name order.
    pub fn to_xml(&self) -> String {
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
        out.push_str("<Types xmlns=\"");
        out.push_str(NAMESPACE);
        out.push_str("\">");
        for (extension, kind) in &self.defaults {
            out.push_str("<Default Extension=\"");
            escape_into(&mut out, extension);
            out.push_str("\" ContentType=\"");
            escape_into(&mut out, kind);
            out.push_str("\"/>");
        }
        for (name, kind) in &self.overrides {
            out.push_str("<Override PartName=\"");
            escape_into(&mut out, name);
            out.push_str("\" ContentType=\"");
            escape_into(&mut out, kind);
            out.push_str("\"/>");
        }
        out.push_str("</Types>");
        out
    }
}

/// An attribute value, unescaped.
fn attribute(element: &quick_xml::events::BytesStart<'_>, name: &str) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.as_ref() == name)
        .and_then(|attribute| quick_xml::escape::unescape(&attribute.value).ok().map(|value| value.into_owned()))
}

/// Appends an attribute value with the characters XML forbids escaped.
pub(crate) fn escape_into(out: &mut String, text: &str) {
    out.push_str(&quick_xml::escape::escape(text));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_overrides_round_trip() {
        let xml = b"<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
                    <Default Extension=\"png\" ContentType=\"image/png\"/>\
                    <Override PartName=\"/word/document.xml\" ContentType=\"a&amp;b\"/></Types>";
        let mut types = ContentTypes::parse(xml).unwrap();
        assert!(!types.ensure_default("png", "image/png"));
        assert!(types.ensure_default("bin", "application/vnd.openxmlformats-officedocument.oleObject"));
        let written = types.to_xml();
        assert!(written.contains("Extension=\"bin\""));
        let mut read_back = ContentTypes::parse(written.as_bytes()).unwrap();
        assert!(read_back.ensure_default("BIN", "application/vnd.openxmlformats-officedocument.oleObject") == false);
        assert!(read_back.ensure_default("png", "image/png") == false);
        // The override survives the round trip with its entity resolved and re-escaped.
        assert!(written.contains("ContentType=\"a&amp;b\""));
    }

    #[test]
    fn empty_part_parses_to_nothing() {
        assert!(ContentTypes::parse(b"").unwrap().ensure_default("png", "image/png"));
    }
}
