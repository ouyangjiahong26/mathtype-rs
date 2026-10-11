//! Values crossing the automation boundary: the `VARIANT`s `IDispatch` hands out and takes in.

use std::ffi::c_void;
use std::mem::ManuallyDrop;

use windows::Win32::System::Ole::{SafeArrayCreateVector, SafeArrayPutElement};
use windows::Win32::System::Variant::{VARIANT, VARENUM, VT_ARRAY, VT_BSTR, VT_R8};

/// An empty value, which is what a call that only has a side effect returns.
pub fn empty() -> VARIANT {
    VARIANT::default()
}

/// A string value, owned by whoever receives it.
pub fn text(value: &str) -> VARIANT {
    let mut variant = VARIANT::default();
    unsafe {
        let fields = &mut *variant.Anonymous.Anonymous;
        fields.vt = VT_BSTR;
        fields.Anonymous.bstrVal = ManuallyDrop::new(windows::core::BSTR::from(value));
    }
    variant
}

/// A value holding an array of numbers, which is how a size is reported.
pub fn numbers(values: &[f64]) -> VARIANT {
    unsafe {
        let array = SafeArrayCreateVector(VT_R8, 0, values.len() as u32);
        if array.is_null() {
            return empty();
        }
        for (index, value) in values.iter().enumerate() {
            let at = index as i32;
            if SafeArrayPutElement(array, &at, std::ptr::addr_of!(*value).cast::<c_void>()).is_err() {
                return empty();
            }
        }
        let mut variant = VARIANT::default();
        let fields = &mut *variant.Anonymous.Anonymous;
        fields.vt = VARENUM(VT_ARRAY.0 | VT_R8.0);
        fields.Anonymous.parray = array;
        variant
    }
}

/// The string an argument carries, when it carries one.
pub fn as_text(value: *const VARIANT) -> Option<String> {
    if value.is_null() {
        return None;
    }
    unsafe {
        let fields = &*(*value).Anonymous.Anonymous;
        if fields.vt != VT_BSTR {
            return None;
        }
        let text: &windows::core::BSTR = &fields.Anonymous.bstrVal;
        Some(text.to_string())
    }
}

/// The argument at `index`, counting from the left the way a caller writes it.
///
/// `DISPPARAMS` stores arguments right to left, which is the one surprising thing about it. A
/// null pointer means the caller did not pass that argument.
pub fn argument(params: *const windows::Win32::System::Com::DISPPARAMS, index: usize) -> *const VARIANT {
    if params.is_null() {
        return std::ptr::null();
    }
    unsafe {
        let params = &*params;
        if index >= params.cArgs as usize || params.rgvarg.is_null() {
            return std::ptr::null();
        }
        params.rgvarg.add(params.cArgs as usize - 1 - index)
    }
}

#[cfg(test)]
mod tests {
    use super::{as_text, argument, numbers, text};
    use windows::Win32::System::Com::DISPPARAMS;
    use windows::Win32::System::Ole::SafeArrayGetElement;
    use windows::Win32::System::Variant::{VT_BSTR, VariantClear};

    #[test]
    fn a_string_value_reads_back() {
        let value = text("\\frac{a}{b}");
        assert_eq!(as_text(&value).as_deref(), Some("\\frac{a}{b}"));
        unsafe {
            assert_eq!(value.Anonymous.Anonymous.vt, VT_BSTR);
            // The value owns a `BSTR`, so it has to be released rather than dropped.
            VariantClear(&value as *const _ as *mut _).unwrap();
        }
    }

    #[test]
    fn a_number_array_has_both_numbers() {
        let value = numbers(&[12.0, 34.0]);
        unsafe {
            let array = value.Anonymous.Anonymous.Anonymous.parray;
            assert!(!array.is_null());
            let mut element = 0f64;
            let at = 1i32;
            SafeArrayGetElement(array, &at, std::ptr::addr_of_mut!(element).cast()).unwrap();
            assert_eq!(element, 34.0);
            VariantClear(&value as *const _ as *mut _).unwrap();
        }
    }

    #[test]
    fn an_argument_is_read_from_the_right_end_of_the_list() {
        // A caller writes `first, second`; `DISPPARAMS` stores them the other way round.
        let mut arguments = [text("second"), text("first")];
        let params = DISPPARAMS {
            rgvarg: arguments.as_mut_ptr(),
            rgdispidNamedArgs: std::ptr::null_mut(),
            cArgs: 2,
            cNamedArgs: 0,
        };
        assert_eq!(as_text(argument(&params, 0)).as_deref(), Some("first"));
        assert_eq!(as_text(argument(&params, 1)).as_deref(), Some("second"));
        assert!(argument(&params, 2).is_null());
        for value in &arguments {
            unsafe { VariantClear(value as *const _ as *mut _).unwrap() };
        }
    }
}
