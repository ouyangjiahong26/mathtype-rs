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

/// `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`, the spelling the registry uses.
pub fn clsid_string() -> String {
    let id = CLSID_EQUATION;
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
}
