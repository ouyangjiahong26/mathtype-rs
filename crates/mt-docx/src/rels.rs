//! Relationship parts (`*.rels`): the id→target table that links a part to the parts it uses.

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::content_types::escape_into;
use crate::error::{DocxError, xml_error};

/// The namespace of a relationship part.
const NAMESPACE: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// A relationship type: what a document part points at. The three this crate creates itself.
pub mod kinds {
    /// An image part, pointed at by `r:embed` on a `a:blip` or `r:id` on `v:imagedata`.
    pub const IMAGE: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
    /// An embedded OLE object part, pointed at by `r:id` on `o:OLEObject`.
    pub const OLE_OBJECT: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject";
}

/// One relationship: an id, the kind of thing it points at, and where that thing is.
#[derive(Clone, Debug, PartialEq)]
pub struct Relationship {
    /// The id markup refers to, `rId7` for instance.
    pub id: String,
    /// The relationship type URI.
    pub kind: String,
    /// The target, relative to the part that owns this relationship part.
    pub target: String,
    /// Whether the target lives outside the container (`TargetMode="External"`).
    pub external: bool,
}

/// The relationships of one part, in part order.
#[derive(Default)]
pub struct Relationships {
    rels: Vec<Relationship>,
}

impl Relationships {
    /// Parses a relationship part; an empty byte string is an empty set.
    pub fn parse(bytes: &[u8]) -> Result<Self, DocxError> {
        if bytes.is_empty() {
            return Ok(Self::default());
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| DocxError::NotUtf8("relationships part".to_owned()))?;
        let mut reader = Reader::from_str(text);
        reader.config_mut().trim_text(false);
        let mut rels = Vec::new();
        loop {
            let event = reader.read_event().map_err(xml_error)?;
            let element = match &event {
                Event::Start(start) | Event::Empty(start) => start,
                Event::Eof => break,
                _ => continue,
            };
            if element.local_name().as_ref() != "Relationship" {
                continue;
            }
            let attribute = |name: &str| {
                element
                    .attributes()
                    .flatten()
                    .find(|attribute| attribute.key.as_ref() == name)
                    .and_then(|attribute| {
                        quick_xml::escape::unescape(&attribute.value)
                            .ok()
                            .map(|value| value.into_owned())
                    })
            };
            rels.push(Relationship {
                id: attribute("Id").unwrap_or_default(),
                kind: attribute("Type").unwrap_or_default(),
                target: attribute("Target").unwrap_or_default(),
                external: attribute("TargetMode").as_deref() == Some("External"),
            });
        }
        Ok(Self { rels })
    }

    /// The relationships, in part order.
    pub fn iter(&self) -> impl Iterator<Item = &Relationship> {
        self.rels.iter()
    }

    /// The relationship with this id.
    pub fn by_id(&self, id: &str) -> Option<&Relationship> {
        self.rels.iter().find(|rel| rel.id == id)
    }

    /// The first relationship of this kind pointing at this target.
    pub fn find(&self, kind: &str, target: &str) -> Option<&Relationship> {
        self.rels
            .iter()
            .find(|rel| rel.kind == kind && rel.target == target)
    }

    /// Adds a relationship, reusing an existing one that already points there. Returns the id.
    pub fn add(&mut self, kind: &str, target: &str, external: bool) -> String {
        if let Some(existing) = self.find(kind, target) {
            return existing.id.clone();
        }
        let id = self.next_id();
        self.rels.push(Relationship {
            id: id.clone(),
            kind: kind.to_owned(),
            target: target.to_owned(),
            external,
        });
        id
    }

    /// Drops the relationship with this id, if there is one.
    pub fn remove(&mut self, id: &str) {
        self.rels.retain(|rel| rel.id != id);
    }

    /// The first free `rIdN`, continuing the numbering the part already uses.
    fn next_id(&self) -> String {
        let highest = self
            .rels
            .iter()
            .filter_map(|rel| rel.id.strip_prefix("rId"))
            .filter_map(|digits| digits.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("rId{}", highest + 1)
    }

    /// Serialises the part.
    pub fn to_xml(&self) -> String {
        let mut out =
            String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
        out.push_str("<Relationships xmlns=\"");
        out.push_str(NAMESPACE);
        out.push_str("\">");
        for rel in &self.rels {
            out.push_str("<Relationship Id=\"");
            escape_into(&mut out, &rel.id);
            out.push_str("\" Type=\"");
            escape_into(&mut out, &rel.kind);
            out.push_str("\" Target=\"");
            escape_into(&mut out, &rel.target);
            out.push_str("\"");
            if rel.external {
                out.push_str(" TargetMode=\"External\"");
            }
            out.push_str("/>");
        }
        out.push_str("</Relationships>");
        out
    }
}

/// Resolves a relationship target against the directory of the part that owns the relationship.
///
/// `word/_rels/document.xml.rels` targets are relative to `word/`, so `../media/x.png` resolves
/// to `word/media/x.png`. External targets come back unchanged.
pub fn resolve(base_dir: &str, target: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut source = base_dir
        .split('/')
        .filter(|part| !part.is_empty())
        .peekable();
    let mut walking = target.split('/');
    let absolute = target.starts_with('/');
    if !absolute {
        parts.extend(source.by_ref());
    }
    for step in walking.by_ref() {
        match step {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_continue_the_existing_numbering() {
        let xml = b"<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
                    <Relationship Id=\"rId2\" Type=\"http://x/image\" Target=\"media/a.png\"/>\
                    <Relationship Id=\"rId10\" Type=\"http://x/oleObject\" Target=\"embeddings/a.bin\"/></Relationships>";
        let mut rels = Relationships::parse(xml).unwrap();
        assert_eq!(rels.add("http://x/image", "media/b.png", false), "rId11");
        assert_eq!(rels.add("http://x/image", "media/a.png", false), "rId2");
        rels.remove("rId2");
        let read_back = Relationships::parse(rels.to_xml().as_bytes()).unwrap();
        assert_eq!(read_back.iter().count(), 2);
        assert!(read_back.by_id("rId2").is_none());
    }

    #[test]
    fn targets_resolve_against_the_owning_part() {
        assert_eq!(resolve("word", "../media/x.png"), "media/x.png");
        assert_eq!(resolve("word", "media/x.png"), "word/media/x.png");
        assert_eq!(resolve("", "word/document.xml"), "word/document.xml");
        assert_eq!(resolve("word", "/abs/target"), "abs/target");
    }
}
