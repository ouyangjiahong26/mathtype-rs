//! Late-bound access to a COM object through `IDispatch`.
//!
//! The add-in never links the Word type library: the host hands us its `Application` object when
//! it connects, and every later call is a name lookup followed by an invoke. All Word interaction
//! in this crate goes through [`Object`].

use std::ffi::c_void;
use std::mem::{self, ManuallyDrop};

use windows::Win32::Foundation::{DISP_E_PARAMNOTFOUND, DISP_E_UNKNOWNNAME, E_FAIL, VARIANT_BOOL};
use windows::Win32::System::Com::{
    DISPATCH_FLAGS, DISPATCH_METHOD, DISPATCH_PROPERTYGET, DISPATCH_PROPERTYPUT, DISPPARAMS,
    IDispatch,
};
use windows::Win32::System::Variant::{
    VARENUM, VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL, VT_BSTR, VT_DISPATCH,
    VT_EMPTY, VT_ERROR, VT_I2, VT_I4, VT_NULL, VT_R4, VT_R8, VariantClear,
};
use windows::core::{BSTR, Error, GUID, Interface, PCWSTR, Result};

/// `DISPID_PROPERTYPUT` from `oaidl.h`: names the value argument of a property put.
const DISPID_PROPERTYPUT: i32 = -3;

/// The neutral locale; every member name used here is a plain English one.
const LOCALE_NEUTRAL: u32 = 0;

/// A `VARIANT` that releases whatever it holds when it is dropped.
///
/// Values arriving from the host are owned by the caller of [`Object`], which is why they are
/// always handed back to the system through `VariantClear` rather than used in place.
pub struct Variant(VARIANT);

impl Drop for Variant {
    fn drop(&mut self) {
        // SAFETY: this variant was built by this module and is cleared exactly once, here.
        unsafe {
            let _ = VariantClear(&mut self.0);
        }
    }
}

impl Variant {
    /// An empty variant, the usual value of an optional argument.
    pub fn empty() -> Self {
        Self(VARIANT::default())
    }

    /// A 32 bit integer.
    pub fn i32(value: i32) -> Self {
        Self::holding(VT_I4, VARIANT_0_0_0 { lVal: value })
    }

    /// A double precision number; tab stops and positions live in points.
    pub fn r8(value: f64) -> Self {
        Self::holding(VT_R8, VARIANT_0_0_0 { dblVal: value })
    }

    /// A string; the variant owns a copy of it.
    pub fn bstr(value: &str) -> Self {
        Self::holding(
            VT_BSTR,
            VARIANT_0_0_0 {
                bstrVal: ManuallyDrop::new(BSTR::from(value)),
            },
        )
    }

    /// A boolean.
    pub fn boolean(value: bool) -> Self {
        Self::holding(
            VT_BOOL,
            VARIANT_0_0_0 {
                boolVal: VARIANT_BOOL(if value { -1 } else { 0 }),
            },
        )
    }

    /// The marker for an optional argument the caller leaves out.
    pub fn missing() -> Self {
        Self::holding(
            VT_ERROR,
            VARIANT_0_0_0 {
                scode: DISP_E_PARAMNOTFOUND.0,
            },
        )
    }

    /// An interface pointer; the variant takes one reference of its own.
    pub fn dispatch(value: &IDispatch) -> Self {
        Self::holding(
            VT_DISPATCH,
            VARIANT_0_0_0 {
                pdispVal: ManuallyDrop::new(Some(value.clone())),
            },
        )
    }

    /// A variant with `vt` as its discriminant and `payload` as its value.
    ///
    /// The union is built whole, so no reference to its `ManuallyDrop` fields is ever formed; the
    /// [`Drop`] implementation releases whatever the payload owns.
    fn holding(vt: VARENUM, payload: VARIANT_0_0_0) -> Self {
        Self(VARIANT {
            Anonymous: VARIANT_0 {
                Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                    vt,
                    Anonymous: payload,
                    ..Default::default()
                }),
            },
        })
    }

    /// A string value, or the empty string for the two "no value" variant types.
    pub fn string(&self) -> Result<String> {
        let vt = variant_type(&self.0);
        if vt == VT_BSTR {
            // SAFETY: the discriminant is `VT_BSTR`, so `bstrVal` is the live arm.
            unsafe { Ok(variant_payload(&self.0).bstrVal.to_string()) }
        } else if vt == VT_EMPTY || vt == VT_NULL {
            Ok(String::new())
        } else {
            Err(Error::from(E_FAIL))
        }
    }

    /// A numeric value, accepting the integer and floating point types Office uses.
    pub fn number(&self) -> Result<f64> {
        let vt = variant_type(&self.0);
        // SAFETY: each arm below reads the union arm its discriminant selects.
        unsafe {
            let payload = variant_payload(&self.0);
            if vt == VT_I4 {
                Ok(payload.lVal as f64)
            } else if vt == VT_I2 {
                Ok(payload.iVal as f64)
            } else if vt == VT_R4 {
                Ok(payload.fltVal as f64)
            } else if vt == VT_R8 {
                Ok(payload.dblVal)
            } else if vt == VT_BOOL {
                Ok(if payload.boolVal == VARIANT_BOOL(-1) {
                    1.0
                } else {
                    0.0
                })
            } else {
                Err(Error::from(E_FAIL))
            }
        }
    }

    /// An interface pointer, taking a reference of its own.
    pub fn object(&self) -> Result<IDispatch> {
        if variant_type(&self.0) != VT_DISPATCH {
            return Err(Error::from(E_FAIL));
        }
        // SAFETY: the discriminant is `VT_DISPATCH`, so `pdispVal` is the live arm.
        unsafe { object_of(variant_payload(&self.0)) }.ok_or_else(|| Error::from(E_FAIL))
    }

    /// Moves the raw variant out without clearing it.
    pub(crate) fn into_inner(self) -> VARIANT {
        // SAFETY: the value is moved out and the original is forgotten, so the `Drop` that would
        // clear it never runs.
        let value = unsafe { std::ptr::read(&self.0) };
        mem::forget(self);
        value
    }
}

/// Reads the interface pointer out of a variant that is still owned by the host.
///
/// The variant itself is left untouched; the returned interface holds a reference of its own.
pub(crate) fn borrowed_object(raw: &VARIANT) -> Option<IDispatch> {
    if variant_type(raw) != VT_DISPATCH {
        return None;
    }
    // SAFETY: the discriminant is `VT_DISPATCH`, so `pdispVal` is the live arm.
    unsafe { object_of(variant_payload(raw)) }
}

/// Takes a reference to the interface a payload arm holds.
///
/// # Safety
///
/// The caller must have checked that the payload holds an interface pointer.
unsafe fn object_of(payload: &VARIANT_0_0_0) -> Option<IDispatch> {
    // SAFETY: the caller guarantees the arm; the reference is only cloned, never cleared.
    unsafe {
        let value: &Option<IDispatch> = &payload.pdispVal;
        value.clone()
    }
}

/// The discriminant of a variant.
fn variant_type(value: &VARIANT) -> VARENUM {
    // SAFETY: reading the discriminant of a variant is always valid, whatever it holds.
    unsafe { value.Anonymous.Anonymous.vt }
}

/// The value arm of a variant.
///
/// # Safety
///
/// The caller must have checked the discriminant for the arm it reads.
unsafe fn variant_payload(value: &VARIANT) -> &VARIANT_0_0_0 {
    // SAFETY: the caller checked the discriminant.
    unsafe { &value.Anonymous.Anonymous.Anonymous }
}

/// A COM object reached through late binding.
pub struct Object(IDispatch);

impl Object {
    /// The interface pointer this wraps, for arguments that must carry the object itself.
    pub fn as_dispatch(&self) -> &IDispatch {
        &self.0
    }

    /// Wraps an interface pointer that is already owned.
    pub fn from_dispatch(dispatch: IDispatch) -> Self {
        Self(dispatch)
    }

    /// Wraps an interface pointer borrowed from the host, taking a reference of our own.
    ///
    /// # Safety
    ///
    /// `pointer` must be null or point to a live `IDispatch`.
    pub unsafe fn from_pointer(pointer: *mut c_void) -> Option<Self> {
        // SAFETY: the caller guarantees the pointer, if any, is a live dispatch pointer.
        unsafe { IDispatch::from_raw_borrowed(&pointer) }.map(|value| Self(value.clone()))
    }

    /// The underlying dispatch pointer, handing over our reference.
    pub fn into_dispatch(self) -> IDispatch {
        self.0
    }

    /// The member id of `name`, or `DISP_E_UNKNOWNNAME` when the object has no such member.
    fn dispid(&self, name: &str) -> Result<i32> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let mut dispid = 0;
        // SAFETY: the name is NUL terminated and outlives the call; `rgdispid` is a valid slot.
        unsafe {
            self.0.GetIDsOfNames(
                &GUID::zeroed(),
                &PCWSTR(wide.as_ptr()),
                1,
                LOCALE_NEUTRAL,
                &mut dispid,
            )?;
        }
        Ok(dispid)
    }

    /// The result of invoking `dispid`.
    fn invoke(
        &self,
        dispid: i32,
        flags: DISPATCH_FLAGS,
        arguments: Vec<Variant>,
    ) -> Result<Variant> {
        // `DISPPARAMS` carries its arguments in reverse order.
        let mut raw: Vec<VARIANT> = arguments
            .into_iter()
            .rev()
            .map(Variant::into_inner)
            .collect();
        let parameters = DISPPARAMS {
            rgvarg: raw.as_mut_ptr(),
            rgdispidNamedArgs: std::ptr::null_mut(),
            cArgs: raw.len() as u32,
            cNamedArgs: 0,
        };
        let mut result = Variant::empty();
        // SAFETY: the parameters describe `raw`, which outlives the call, and `result` is a valid
        // slot. The callee borrows the arguments and writes at most one result.
        let outcome = unsafe {
            self.0.Invoke(
                dispid,
                &GUID::zeroed(),
                LOCALE_NEUTRAL,
                flags,
                &parameters,
                Some(&mut result.0 as *mut VARIANT),
                None,
                None,
            )
        };
        // The arguments were lent to the callee, not consumed, so we clear them ourselves.
        for argument in raw.iter_mut() {
            // SAFETY: each entry came from a `Variant` and has not been cleared yet.
            unsafe {
                let _ = VariantClear(argument);
            }
        }
        outcome.map(|()| result)
    }

    /// Reads a property.
    pub fn get(&self, name: &str) -> Result<Variant> {
        let dispid = self.dispid(name)?;
        self.invoke(dispid, DISPATCH_PROPERTYGET, Vec::new())
    }

    /// Reads a property that holds an object.
    pub fn get_object(&self, name: &str) -> Result<Self> {
        self.get(name)?.object().map(Self)
    }

    /// Reads a property that holds a string.
    pub fn get_string(&self, name: &str) -> Result<String> {
        self.get(name)?.string()
    }

    /// Reads a property that holds a whole number.
    pub fn get_i32(&self, name: &str) -> Result<i32> {
        self.get(name)?.number().map(|value| value as i32)
    }

    /// Calls a method or a parameterised property.
    pub fn call(&self, name: &str, arguments: Vec<Variant>) -> Result<Variant> {
        let dispid = self.dispid(name)?;
        // Word spells parameterised properties such as `Range(Start, End)` as calls, so both
        // invoke kinds are offered.
        self.invoke(dispid, DISPATCH_METHOD | DISPATCH_PROPERTYGET, arguments)
    }

    /// Calls a method that returns an object.
    pub fn call_object(&self, name: &str, arguments: Vec<Variant>) -> Result<Self> {
        self.call(name, arguments)?.object().map(Self)
    }

    /// Writes a property.
    pub fn put(&self, name: &str, value: Variant) -> Result<()> {
        let dispid = self.dispid(name)?;
        let mut raw = [value.into_inner()];
        // A property put sends its value as a named argument: `DISPID_PROPERTYPUT`.
        let mut named = [DISPID_PROPERTYPUT];
        let parameters = DISPPARAMS {
            rgvarg: raw.as_mut_ptr(),
            rgdispidNamedArgs: named.as_mut_ptr(),
            cArgs: 1,
            cNamedArgs: 1,
        };
        // SAFETY: the parameters describe `raw`, which outlives the call; no result is written.
        let outcome = unsafe {
            self.0.Invoke(
                dispid,
                &GUID::zeroed(),
                LOCALE_NEUTRAL,
                DISPATCH_PROPERTYPUT,
                &parameters,
                None,
                None,
                None,
            )
        };
        // SAFETY: the value was lent to the callee and has not been cleared yet.
        let _ = unsafe { VariantClear(&mut raw[0]) };
        outcome
    }

    /// Writes a string property.
    pub fn put_string(&self, name: &str, value: &str) -> Result<()> {
        self.put(name, Variant::bstr(value))
    }
}

/// The member id reported for a name this object does not have.
pub fn unknown_name() -> Error {
    Error::from(DISP_E_UNKNOWNNAME)
}
