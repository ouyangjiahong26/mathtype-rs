//! The clipboard payload that lets Word build a native equation from our Office Math markup.
//!
//! Word's HTML importer picks the equation branch of an `msEquation` conditional comment and
//! builds an equation from the markup inside it, so the markup has to be the HTML spelling of
//! Office Math rather than the pure OOXML one.

use windows::Win32::Foundation::{E_FAIL, GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::Globalization::{CP_ACP, WideCharToMultiByte};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::core::{Error, PCSTR, Result, w};

/// The Office Math namespace Office's HTML importer recognises.
pub const HTML_MATH_NAMESPACE: &str = "http://schemas.microsoft.com/office/2004/12/omml";

/// The WordprocessingML namespace Word pairs it with, declared for completeness.
const HTML_WORD_NAMESPACE: &str = "urn:schemas-microsoft-com:office:word";

/// `CF_UNICODETEXT`.
const CF_UNICODETEXT: u32 = 13;

/// `CF_TEXT`, the legacy text format, which spells the text in the system's own encoding.
const CF_TEXT: u32 = 1;

/// Start of the fragment inside the generated HTML.
const FRAGMENT_START: &str = "<!--StartFragment-->";

/// End of the fragment inside the generated HTML.
const FRAGMENT_END: &str = "<!--EndFragment-->";

/// Opens the branch of the document that holds the equation markup.
const EQUATION_OPEN: &str = "<!--[if gte msEquation 12]>";

/// Closes the equation branch; everything inside is skipped by consumers that are not Word.
const EQUATION_CLOSE: &str = "<![endif]-->";

/// Opens the branch that consumers without equation support see.
const FALLBACK_OPEN: &str = "<![if !msEquation]>";

/// Closes the fallback branch.
const FALLBACK_CLOSE: &str = "<![endif]>";

/// Puts an equation on the clipboard, ready to be pasted into the selection.
///
/// `omml` is an `<m:oMath>` fragment and `latex` the plain text fallback. The clipboard also gets
/// `CF_UNICODETEXT`, so applications that cannot take the markup see the LaTeX source.
pub fn put_equation(omml: &str, latex: &str) -> Result<()> {
    let markup = omml.replace(mt_omml::NAMESPACE, HTML_MATH_NAMESPACE);
    let fragment = format!(
        "{EQUATION_OPEN}{markup}{EQUATION_CLOSE}{FALLBACK_OPEN}<span>{}</span>{FALLBACK_CLOSE}",
        escape(latex)
    );
    let html = cf_html(&fragment);
    let text: Vec<u8> = latex
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect();

    // SAFETY: the clipboard is opened, filled with moveable memory that the system takes over,
    // and closed again before returning.
    unsafe {
        OpenClipboard(None)?;
        let outcome = fill(&[
            (RegisterClipboardFormatW(w!("HTML Format")), html.as_bytes()),
            (CF_UNICODETEXT, &text),
        ]);
        let _ = CloseClipboard();
        outcome
    }
}

/// Puts the LaTeX source on the clipboard on its own.
///
/// Creating an equation object gives its server no chance to take the source through the call, so
/// the source is left on the clipboard for the server to pick up as it is created. Both the
/// current and the legacy text formats are offered, so the server sees the source whichever it
/// reads.
pub fn put_latex(latex: &str) -> Result<()> {
    let unicode: Vec<u8> = latex
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    let ansi = ansi_bytes(latex);
    // SAFETY: the clipboard is opened, filled with moveable memory that the system takes over, and
    // closed again before returning.
    unsafe {
        OpenClipboard(None)?;
        let outcome = fill(&[(CF_UNICODETEXT, &unicode), (CF_TEXT, &ansi)]);
        let _ = CloseClipboard();
        outcome
    }
}

/// The text in the system's own encoding, with its terminating NUL.
fn ansi_bytes(text: &str) -> Vec<u8> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: the first call only measures, and the second writes into a buffer of that size.
    unsafe {
        let length = WideCharToMultiByte(CP_ACP, 0, &wide, None, PCSTR::null(), None);
        if length <= 0 {
            return Vec::new();
        }
        let mut bytes = vec![0u8; length as usize];
        let written = WideCharToMultiByte(CP_ACP, 0, &wide, Some(&mut bytes), PCSTR::null(), None);
        bytes.truncate(written.max(0) as usize);
        bytes
    }
}

/// Builds the `HTML Format` payload: a byte-offset header followed by the markup.
fn cf_html(fragment: &str) -> String {
    // The offsets are byte counts from the start of the payload, and the header has a fixed width,
    // so the fragment position can be computed before the header itself is written.
    let html = format!(
        "<html xmlns:m=\"{HTML_MATH_NAMESPACE}\" xmlns:w=\"{HTML_WORD_NAMESPACE}\">\
         <head><meta http-equiv=Content-Type content=\"text/html; charset=utf-8\"></head><body>\
         {FRAGMENT_START}{fragment}{FRAGMENT_END}</body></html>"
    );
    let blank = header(0, 0, 0, 0);
    let start_html = blank.len();
    let start_fragment = start_html
        + html.find(FRAGMENT_START).expect("marker is written above")
        + FRAGMENT_START.len();
    let header = header(
        start_html,
        start_html + html.len(),
        start_fragment,
        start_fragment + fragment.len(),
    );
    format!("{header}{html}")
}

/// The `HTML Format` header, with the offsets written as ten decimal digits.
fn header(
    start_html: usize,
    end_html: usize,
    start_fragment: usize,
    end_fragment: usize,
) -> String {
    format!(
        "Version:1.0\r\nStartHTML:{start_html:010}\r\nEndHTML:{end_html:010}\r\n\
         StartFragment:{start_fragment:010}\r\nEndFragment:{end_fragment:010}\r\n"
    )
}

/// Replaces the characters that may not appear literally in HTML text.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Hands every entry to the clipboard, which takes ownership of the memory.
///
/// # Safety
///
/// The clipboard must be open.
unsafe fn fill(entries: &[(u32, &[u8])]) -> Result<()> {
    // SAFETY: the caller opened the clipboard for this thread.
    unsafe { EmptyClipboard()? };
    for (format, bytes) in entries {
        // SAFETY: the clipboard is open, so allocating and handing over memory is valid.
        let handle = unsafe { allocate(bytes)? };
        // SAFETY: `handle` is a freshly allocated moveable block the system takes over on success.
        if unsafe { SetClipboardData(*format, Some(HANDLE(handle.0))) }.is_err() {
            // SAFETY: the system did not take the block, so it is still ours to free.
            unsafe {
                let _ = GlobalFree(Some(handle));
            }
            return Err(Error::from(E_FAIL));
        }
    }
    Ok(())
}

/// A moveable block holding a copy of `bytes`.
///
/// # Safety
///
/// The caller owns the returned handle and must either free it or hand it to the clipboard.
unsafe fn allocate(bytes: &[u8]) -> Result<HGLOBAL> {
    // SAFETY: the allocation is checked before it is locked and written through.
    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
        let target = GlobalLock(handle);
        if target.is_null() {
            let _ = GlobalFree(Some(handle));
            return Err(Error::from(E_FAIL));
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(handle);
        Ok(handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_offsets_point_at_the_fragment() {
        let html = cf_html("<m:oMath>x</m:oMath>");
        let field = |name: &str| -> usize {
            html.lines()
                .find_map(|line| line.strip_prefix(name))
                .unwrap_or_else(|| panic!("{name} header line is present"))
                .parse()
                .expect("the offset is a number")
        };
        let start = field("StartFragment:");
        let end = field("EndFragment:");
        assert_eq!(&html[start..end], "<m:oMath>x</m:oMath>");
        assert_eq!(
            field("StartHTML:"),
            html.find("<html")
                .expect("the html element follows the header")
        );
        assert_eq!(field("EndHTML:"), html.len());
    }

    #[test]
    fn html_offsets_are_bytes_not_characters() {
        // The equation markup holds characters outside ASCII, and the offsets must still address
        // the fragment: Word reads the payload as UTF-8 bytes.
        let html = cf_html("<m:r><m:t>α</m:t></m:r>");
        let start: usize = html
            .lines()
            .find_map(|line| line.strip_prefix("StartFragment:"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            &html[start..start + "<m:r><m:t>α</m:t></m:r>".len()],
            "<m:r><m:t>α</m:t></m:r>"
        );
    }

    #[test]
    fn equation_payload_keeps_the_markup_and_the_namespace_word_wants() {
        let html = cf_html("fragment");
        assert!(html.contains(HTML_MATH_NAMESPACE));
        assert!(html.contains(FRAGMENT_START));
    }

    #[test]
    fn escaping_removes_markup_characters() {
        assert_eq!(escape(r"a<&b>"), "a&lt;&amp;b&gt;");
    }
}
