//! The parts of the Word object model the add-in touches.
//!
//! Everything here is late bound: the names are the ones the Word object model documents, and the
//! calls are the ones the ribbon commands need.

use windows::core::Result;

use crate::com::{Object, Variant};

/// The selection of the active document.
pub fn selection(application: &Object) -> Result<Object> {
    application.get_object("Selection")
}

/// The active document.
pub fn active_document(application: &Object) -> Result<Object> {
    application.get_object("ActiveDocument")
}

/// The text of a range or selection.
pub fn text(range: &Object) -> Result<String> {
    range.get_string("Text")
}

/// The `OMaths` collection of anything that has one: a range, a selection, a paragraph.
pub fn equations(scope: &Object) -> Result<Object> {
    scope.get_object("OMaths")
}

/// How many items a collection holds.
pub fn count(collection: &Object) -> Result<i32> {
    collection.get_i32("Count")
}

/// One item of a collection, numbered from one.
pub fn item(collection: &Object, index: i32) -> Result<Object> {
    collection.call_object("Item", vec![Variant::i32(index)])
}

/// The first equation the selection reaches, if it reaches one.
///
/// A selection that only sits at the edge of an equation reports no equation of its own, so the
/// paragraph it is in is consulted as well; that makes a caret next to an equation work the same
/// as a selection that covers it.
pub fn selected_equation(selection: &Object) -> Result<Option<Object>> {
    let direct = equations(selection)?;
    if count(&direct)? > 0 {
        return Ok(Some(item(&direct, 1)?));
    }
    let paragraphs = selection.get_object("Paragraphs")?;
    let paragraph = item(&paragraphs, 1)?;
    let range = paragraph.get_object("Range")?;
    let nearby = equations(&range)?;
    if count(&nearby)? > 0 { Ok(Some(item(&nearby, 1)?)) } else { Ok(None) }
}

/// Replaces an equation with plain text.
///
/// Writing into an equation's own range keeps the equation, its text merely becomes mathematical
/// text, so the equation is removed first and the text is put at the position it occupied.
pub fn replace_equation(equation: &Object, text: &str) -> Result<()> {
    let range = equation.get_object("Range")?;
    range.call("Delete", Vec::new())?;
    range.call("InsertAfter", vec![Variant::bstr(text)])?;
    Ok(())
}

/// The `m:oMath` element of a WordprocessingML document, if it has one.
///
/// A range's `WordOpenXML` is a whole document, so the equation is cut out of it. A displayed
/// equation is wrapped in `m:oMathPara`, which is taken whole; an inline one is taken as is.
pub fn extract_math(xml: &str) -> Option<&str> {
    const OPEN: &str = "<m:oMath";
    let mut search = 0;
    while let Some(found) = xml[search..].find(OPEN) {
        let start = search + found;
        let tail = &xml[start + OPEN.len()..];
        let tag = if tail.starts_with("Para") {
            "m:oMathPara"
        } else if tail.starts_with('>') || tail.starts_with(char::is_whitespace) {
            "m:oMath"
        } else {
            // A longer element name that merely starts the same way.
            search = start + OPEN.len();
            continue;
        };
        let close = format!("</{tag}>");
        let end = xml[start..].find(&close)? + close.len();
        return Some(&xml[start..start + end]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::extract_math;

    #[test]
    fn an_inline_equation_is_taken_with_its_closing_tag() {
        let xml = "<w:body><w:p><m:oMath><m:r><m:t>a</m:t></m:r></m:oMath></w:p></w:body>";
        assert_eq!(extract_math(xml), Some("<m:oMath><m:r><m:t>a</m:t></m:r></m:oMath>"));
    }

    #[test]
    fn a_displayed_equation_is_taken_whole() {
        let xml = "<w:p><m:oMathPara><m:oMath><m:r><m:t>a</m:t></m:r></m:oMath></m:oMathPara></w:p>";
        assert_eq!(
            extract_math(xml),
            Some("<m:oMathPara><m:oMath><m:r><m:t>a</m:t></m:r></m:oMath></m:oMathPara>")
        );
    }

    #[test]
    fn an_element_that_merely_shares_the_prefix_is_skipped() {
        let xml = "<m:oMathSomething/><m:oMath>a</m:oMath>";
        assert_eq!(extract_math(xml), Some("<m:oMath>a</m:oMath>"));
    }

    #[test]
    fn a_document_without_an_equation_yields_nothing() {
        assert_eq!(extract_math("<w:p>text</w:p>"), None);
        assert_eq!(extract_math("<m:oMath>unterminated"), None);
    }

    #[test]
    fn an_equation_that_opens_with_attributes_is_found() {
        let xml = r#"<m:oMath xmlns:m="urn:x" m:foo="1">a</m:oMath>"#;
        assert_eq!(extract_math(xml), Some(r#"<m:oMath xmlns:m="urn:x" m:foo="1">a</m:oMath>"#));
    }
}
