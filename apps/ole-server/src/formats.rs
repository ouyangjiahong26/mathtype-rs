//! The one clipboard format the object presents: its equation as a `CF_DIB`.
//!
//! `QueryGetData` and `GetData` share [`supported`], so what the object claims to have is exactly
//! what it can hand over.

use std::cell::Cell;
use std::ptr;

use windows::Win32::Foundation::{DV_E_DVASPECT, DV_E_FORMATETC, DV_E_TYMED, S_FALSE};
use windows::Win32::System::Com::{
    DATADIR_GET, DVASPECT_CONTENT, FORMATETC, IEnumFORMATETC, IEnumFORMATETC_Impl, TYMED_HGLOBAL,
};
use windows::Win32::System::Ole::CF_DIB;
use windows::core::{HRESULT, implement};

/// The presentation the object offers: a device-independent bitmap of the equation.
pub fn dib_format() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_DIB.0,
        ptd: ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        // OLE's "no index"; hosts that spell it zero are accepted too, by `supported`.
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

/// Whether a request describes the presentation the object has.
///
/// `lindex` is `-1` for the whole presentation; a few hosts pass `0` for the same thing, and
/// treating that as a miss costs an object its picture, so both are accepted.
pub fn supported(format: &FORMATETC) -> bool {
    if format.cfFormat != CF_DIB.0 {
        return false;
    }
    if format.dwAspect != DVASPECT_CONTENT.0 || !matches!(format.lindex, -1 | 0) {
        return false;
    }
    format.tymed & TYMED_HGLOBAL.0 as u32 != 0
}

/// Why a request cannot be served, as the `HRESULT` the contract asks for.
pub fn rejection(format: &FORMATETC) -> HRESULT {
    if format.cfFormat != CF_DIB.0 {
        return DV_E_FORMATETC;
    }
    if format.dwAspect != DVASPECT_CONTENT.0 || !matches!(format.lindex, -1 | 0) {
        return DV_E_DVASPECT;
    }
    if format.tymed & TYMED_HGLOBAL.0 as u32 == 0 {
        return DV_E_TYMED;
    }
    DV_E_FORMATETC
}

/// Enumerator over the formats the object can produce.
#[implement(IEnumFORMATETC, Agile = false)]
pub struct FormatEnumerator {
    formats: Vec<FORMATETC>,
    index: Cell<usize>,
}

impl FormatEnumerator {
    /// An enumerator over `formats`, positioned at the first one.
    pub fn new(formats: Vec<FORMATETC>) -> Self {
        Self { formats, index: Cell::new(0) }
    }

    /// The formats the object offers to a host that is asking for data.
    pub fn for_get() -> IEnumFORMATETC {
        Self::new(vec![dib_format()]).into()
    }
}

impl IEnumFORMATETC_Impl for FormatEnumerator_Impl {
    fn Next(&self, celt: u32, rgelt: *mut FORMATETC, pceltfetched: *mut u32) -> HRESULT {
        let start = self.index.get();
        let mut taken = 0usize;
        while taken < celt as usize {
            let Some(format) = self.formats.get(start + taken) else {
                break;
            };
            if !rgelt.is_null() {
                // `FORMATETC` is plain data; handing a host its own copy is the whole contract.
                unsafe { rgelt.add(taken).write(*format) };
            }
            taken += 1;
        }
        self.index.set(start + taken);
        if !pceltfetched.is_null() {
            unsafe { pceltfetched.write(taken as u32) };
        }
        if taken == celt as usize { HRESULT(0) } else { S_FALSE }
    }

    fn Skip(&self, celt: u32) -> windows::core::Result<()> {
        self.index.set((self.index.get() + celt as usize).min(self.formats.len()));
        Ok(())
    }

    fn Reset(&self) -> windows::core::Result<()> {
        self.index.set(0);
        Ok(())
    }

    fn Clone(&self) -> windows::core::Result<IEnumFORMATETC> {
        let copy = FormatEnumerator::new(self.formats.clone());
        copy.index.set(self.index.get());
        Ok(copy.into())
    }
}

/// A direction the object does not implement has no enumerator to hand over.
pub fn unsupported_direction(direction: u32) -> bool {
    direction != DATADIR_GET.0 as u32
}

#[cfg(test)]
mod tests {
    use super::{dib_format, rejection, supported};
    use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
    use windows::Win32::System::Ole::CF_DIB;

    #[test]
    fn the_offered_format_is_the_one_that_is_accepted() {
        assert!(supported(&dib_format()));
    }

    #[test]
    fn the_bitmap_under_an_index_is_still_the_bitmap() {
        // Some hosts ask for the whole presentation as index zero rather than "no index".
        let mut format = dib_format();
        format.lindex = 0;
        assert!(supported(&format));
    }

    #[test]
    fn a_different_format_is_rejected_with_the_reason() {
        let mut text = FORMATETC {
            cfFormat: 1,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        assert!(!supported(&text));
        assert_eq!(rejection(&text).0, super::DV_E_FORMATETC.0);
        // The same bitmap under a storage medium is a different reason again.
        text.cfFormat = CF_DIB.0;
        text.tymed = 0;
        assert_eq!(rejection(&text).0, super::DV_E_TYMED.0);
    }
}
