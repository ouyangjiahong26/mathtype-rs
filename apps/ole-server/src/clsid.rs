//! The class identity this server is registered under, and where the object keeps its content.
//!
//! The identifier below is written out in `install.ps1` as well; `--registration` prints it so the
//! script and this binary can be checked against each other instead of trusting that they agree.

use windows::core::GUID;

/// Class of an embedded equation object.
pub const CLSID_EQUATION: GUID = GUID::from_u128(0x7c3e9a54_1b62_4e7d_9f03_5a8c2d6e1b47);

/// Versioned programmatic identifier, the name a document host asks for.
pub const PROGID: &str = "MathTypeRs.Equation.1";

/// Version-independent programmatic identifier, pointing at [`PROGID`] through `CurVer`.
pub const PROGID_UNVERSIONED: &str = "MathTypeRs.Equation";

/// Name the object shows for itself in the host's interface.
pub const FRIENDLY_NAME: &str = "MathType Rs Equation";

/// Stream holding the equation in the format other equation tools read, header included.
pub const STREAM_NATIVE: &str = "Equation Native";

/// Stream holding the same equation as UTF-8 LaTeX text, which is what the editor reads and
/// writes and what re-opening an object trusts first.
pub const STREAM_LATEX: &str = "MathTypeRs Equation";

/// Stream holding the object's class identity, the one a host reads to name the object and to
/// find its programmatic identifier back in a saved document. Its bytes are the
/// `CompObjStream` structure of [MS-OLEDS] 2.3.7: a 28-byte header every reader ignores, then
/// length-prefixed ANSI strings — the display name, the clipboard format name, and the
/// programmatic identifier, which is the one hosts report as `OLEFormat.ProgID`.
pub const STREAM_COMPOBJ: &str = "\u{1}CompObj";

/// The bytes of [`STREAM_COMPOBJ`], in the shape the OLE storage helpers write.
///
/// The two leading words are the byte-order marker and a version every writer copies and no
/// reader checks ([MS-OLEDS]: "can be set to any arbitrary value"); the marker is spelled the
/// way `ole32` spells it. A string is a length in bytes counting the terminating null, then the
/// null-terminated ANSI text ([MS-OLEDS] 2.1.4).
pub fn compobj() -> Vec<u8> {
    fn ansi(text: &str) -> Vec<u8> {
        let mut bytes = (text.len() as u32 + 1).to_le_bytes().to_vec();
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);
        bytes
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0000_FFFEu32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 20]);
    bytes.extend(ansi(FRIENDLY_NAME).iter().copied());
    bytes.extend(ansi("Embed Source").iter().copied());
    bytes.extend(ansi(PROGID).iter().copied());
    bytes
}

/// `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`, the spelling the registry uses.
pub fn id_string(id: &GUID) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        id.data1,
        id.data2,
        id.data3,
        id.data4[0],
        id.data4[1],
        id.data4[2],
        id.data4[3],
        id.data4[4],
        id.data4[5],
        id.data4[6],
        id.data4[7],
    )
}

/// [`id_string`] for the equation class.
pub fn clsid_string() -> String {
    id_string(&CLSID_EQUATION)
}

/// What this server is registered as, one fact per line.
pub fn registration() -> String {
    [
        format!("clsid={}", clsid_string()),
        format!("progid={PROGID}"),
        format!("progid-unversioned={PROGID_UNVERSIONED}"),
        format!("friendly-name={FRIENDLY_NAME}"),
        format!("native-stream={STREAM_NATIVE}"),
        format!("latex-stream={STREAM_LATEX}"),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{CLSID_EQUATION, clsid_string};

    #[test]
    fn the_class_id_is_written_the_way_the_registry_spells_it() {
        assert_eq!(clsid_string(), "{7C3E9A54-1B62-4E7D-9F03-5A8C2D6E1B47}");
        // The first field keeps every bit, so a short field is padded rather than trimmed away.
        assert_ne!(CLSID_EQUATION.to_u128(), 0);
    }

    #[test]
    fn the_class_information_stream_has_the_documented_shape() {
        let bytes = super::compobj();
        // Header: the ole32 byte-order marker, version 1, twenty ignored bytes.
        assert_eq!(&bytes[..4], &[0xFE, 0xFF, 0x00, 0x00]);
        assert_eq!(&bytes[4..8], &[0x01, 0x00, 0x00, 0x00]);
        assert_eq!(&bytes[8..28], &[0u8; 20]);
        // Then three length-prefixed null-terminated ANSI strings: the display name, the
        // clipboard format, the programmatic identifier.
        let mut at = 28;
        for expected in [super::FRIENDLY_NAME, "Embed Source", super::PROGID] {
            let length = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            assert_eq!(length, expected.len() + 1, "length counts the null");
            assert_eq!(&bytes[at + 4..at + 4 + length - 1], expected.as_bytes());
            assert_eq!(bytes[at + 4 + length - 1], 0);
            at += 4 + length;
        }
        assert_eq!(at, bytes.len(), "nothing follows the strings");
    }
}
