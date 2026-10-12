//! Microsoft Word add-in that drives the equation core from a 公式 ribbon tab.
//!
//! The built DLL is an in-process COM server. Word creates the class through `DllGetClassObject`,
//! asks the object for `IDTExtensibility2` to connect it and for `IRibbonExtensibility` to load the
//! ribbon, and later calls the ribbon callbacks back on the object's `IDispatch`.
//!
//! `install.ps1` registers the class under `HKCU`; [`CLSID_ADDIN`] must match the class id it
//! writes.

#![cfg(windows)]
// The members of the interfaces below keep the spelling the host looks them up by, which is the
// spelling of the Office object model rather than Rust's.
#![allow(non_snake_case)]

mod clipboard;
mod com;
mod commands;
mod ribbon;
mod word;

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, DISP_E_MEMBERNOTFOUND, E_FAIL, E_NOTIMPL,
    E_POINTER, S_FALSE, S_OK,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, DISPATCH_FLAGS, DISPPARAMS, EXCEPINFO, IClassFactory,
    IClassFactory_Impl, IDispatch, IDispatch_Impl, IDispatch_Vtbl, IGlobalInterfaceTable,
    ITypeInfo,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{
    BOOL, BSTR, ComObjectInterface, Error, GUID, HRESULT, IUnknown, Interface, PCWSTR, Ref, Result,
    implement, interface,
};

use com::{Object, Variant};
use commands::{Command, Outcome};

/// The class id the host finds in the registry; `install.ps1` writes the same value.
pub const CLSID_ADDIN: GUID = GUID::from_u128(0x8f6c2b41_5a7d_4e93_b0c1_2d4e6f8a9b3c);

/// The number of live add-in objects and class factories; the server may be unloaded once there
/// are none.
static LIVE_OBJECTS: AtomicIsize = AtomicIsize::new(0);

/// The connection interface every Office add-in implements.
///
/// The interface is a dual interface: its vtable continues `IDispatch`, and its methods also
/// answer to `IDispatch::Invoke` under the member ids in [`ribbon::dispid`].
#[interface("B65AD801-ABAF-11D0-BB8B-00A0C90F2744")]
unsafe trait IDTExtensibility2: IDispatch {
    /// Connects the add-in to the host.
    fn OnConnection(
        &self,
        application: *mut c_void,
        connect_mode: i32,
        add_in_instance: *mut c_void,
        custom: *mut c_void,
    ) -> HRESULT;

    /// Disconnects the add-in from the host.
    fn OnDisconnection(&self, remove_mode: i32, custom: *mut c_void) -> HRESULT;

    /// The set of loaded add-ins changed.
    fn OnAddInsUpdate(&self, custom: *mut c_void) -> HRESULT;

    /// The host finished starting up.
    fn OnStartupComplete(&self, custom: *mut c_void) -> HRESULT;

    /// The host is shutting down.
    fn OnBeginShutdown(&self, custom: *mut c_void) -> HRESULT;
}

/// The interface the host asks for the ribbon markup.
///
/// Like the connection interface it is a dual interface, so the ribbon is also reachable through
/// `IDispatch::Invoke` under the member id [`ribbon::dispid::GET_CUSTOM_UI`].
#[interface("000C0396-0000-0000-C000-000000000046")]
unsafe trait IRibbonExtensibility: IDispatch {
    /// Writes the ribbon markup for `ribbon_id` into `ribbon_xml`.
    fn GetCustomUI(&self, ribbon_id: BSTR, ribbon_xml: *mut BSTR) -> HRESULT;
}

/// The add-in object the host connects to.
///
/// Word keeps one object for the life of the application, so the connection it makes in
/// `OnConnection` is the state the commands work against.
#[implement(IDispatch, IDTExtensibility2, IRibbonExtensibility)]
struct Addin {
    /// The host application, kept while the add-in is connected.
    connection: Mutex<Option<Connection>>,
}

impl Drop for Addin {
    fn drop(&mut self) {
        LIVE_OBJECTS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The connection to the host.
///
/// The host hands its application object over while it connects the add-in, but it may call the
/// add-in back on another thread, and an automation pointer is only usable in the apartment it was
/// handed over in. The object is therefore kept in the process-wide interface table and pulled out
/// again for each call, which gives the calling apartment a pointer it can use.
struct Connection {
    /// The table every apartment can pull the object out of.
    table: IGlobalInterfaceTable,
    /// The entry the application object was registered under.
    cookie: u32,
}

// SAFETY: the table is a process-wide object safe to use from any thread, and the cookie is only
// passed back to it.
unsafe impl Send for Connection {}
unsafe impl Sync for Connection {}

impl Connection {
    /// Takes the application object, valid in the apartment that is calling now.
    fn application(&self) -> Result<Object> {
        let mut raw: *mut c_void = std::ptr::null_mut();
        // SAFETY: the cookie is registered until this connection is dropped, and the out parameter
        // is a valid slot.
        unsafe {
            self.table
                .GetInterfaceFromGlobal(self.cookie, &IDispatch::IID, &mut raw)
        }?;
        // SAFETY: the call succeeded, so the slot holds a counted `IDispatch`.
        Ok(Object::from_dispatch(unsafe { IDispatch::from_raw(raw) }))
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: the cookie was registered once, by `Addin::connect`, and is revoked once here.
        unsafe {
            let _ = self.table.RevokeInterfaceFromGlobal(self.cookie);
        }
    }
}

/// The class id of the process-wide interface table.
const CLSID_STD_GLOBAL_INTERFACE_TABLE: GUID =
    GUID::from_u128(0x00000323_0000_0000_c000_000000000046);

/// Locks a mutex, recovering the value if an earlier call panicked while holding it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Addin {
    /// Remembers the host application and hands the add-in object back to the host.
    ///
    /// `add_in_instance` is the host's own `COMAddIn` object for this add-in. Its `Object` property
    /// is where a host looks for the add-in's interface, so the add-in object is stored there; that
    /// is what `Application.COMAddIns("MathTypeRs.Addin").Object` returns.
    fn connect(
        &self,
        application: Object,
        add_in_instance: Option<Object>,
        dispatch: IDispatch,
    ) -> HRESULT {
        // SAFETY: the table is a process-wide object and the thread has COM initialised because the
        // host called us through it.
        let table: IGlobalInterfaceTable = match unsafe {
            CoCreateInstance(
                &CLSID_STD_GLOBAL_INTERFACE_TABLE,
                None,
                CLSCTX_INPROC_SERVER,
            )
        } {
            Ok(table) => table,
            Err(_) => return E_FAIL,
        };
        // SAFETY: the application object is alive for as long as the host is connected.
        let application = application.into_dispatch();
        let cookie = match unsafe { table.RegisterInterfaceInGlobal(&application, &IDispatch::IID) }
        {
            Ok(cookie) => cookie,
            Err(_) => return E_FAIL,
        };
        *lock(&self.connection) = Some(Connection { table, cookie });
        if let Some(add_in) = add_in_instance {
            // The ribbon works without this, so a host that refuses the object is not treated as a
            // failed connection; automation callers just find no object.
            let _ = add_in.put("Object", Variant::dispatch(&dispatch));
        }
        S_OK
    }

    /// Forgets the host application.
    fn disconnect(&self) {
        *lock(&self.connection) = None;
    }

    /// The host application, while the add-in is connected.
    fn application(&self) -> Option<Object> {
        lock(&self.connection)
            .as_ref()
            .and_then(|connection| connection.application().ok())
    }

    /// Runs a command against the host application.
    ///
    /// A panic inside a command would unwind across the COM boundary, which is undefined, so it is
    /// caught here and reported as a failed command instead.
    fn run(&self, command: Command) -> Outcome {
        let Some(application) = self.application() else {
            return Err("the add-in is not connected to Word".to_string());
        };
        catch_unwind(AssertUnwindSafe(|| command.run(&application)))
            .unwrap_or_else(|_| Err("the command did not finish".to_string()))
    }

    /// Runs a ribbon callback and reports its outcome in the status bar.
    fn callback(&self, command: Command) -> String {
        let message = match self.run(command) {
            Ok(message) => message,
            Err(reason) => format!("公式: {reason}"),
        };
        if let Some(application) = self.application() {
            // The status bar is the only place a ribbon button can report anything, and a failure
            // to write it is not worth failing the command over.
            let _ = application.put_string("StatusBar", &message);
        }
        message
    }
}

/// The dispatch interface of the object itself, which is what the host is handed when it stores or
/// asks for the add-in object.
fn self_dispatch(this: &Addin_Impl) -> IDispatch {
    <Addin_Impl as ComObjectInterface<IDispatch>>::as_interface_ref(this).to_owned()
}

/// The argument of a call at positional `index`, as an interface pointer.
///
/// A host that reaches `IDTExtensibility2` through `IDispatch` passes the same arguments as the
/// vtable methods, in reverse order.
fn argument_object(parameters: *const DISPPARAMS, index: usize) -> Option<IDispatch> {
    if parameters.is_null() {
        return None;
    }
    // SAFETY: the host supplies a parameter block that lives for the whole call.
    let parameters = unsafe { &*parameters };
    if index >= parameters.cArgs as usize || parameters.rgvarg.is_null() {
        return None;
    }
    // Arguments arrive in reverse order, so the first parameter is the last entry.
    // SAFETY: `rgvarg` holds exactly `cArgs` entries, and `index` is in range.
    let argument = unsafe { &*parameters.rgvarg.add(parameters.cArgs as usize - 1 - index) };
    com::borrowed_object(argument)
}

impl Addin_Impl {
    /// The value a member id answers with.
    fn dispatch_invoke(&self, dispid: i32, parameters: *const DISPPARAMS) -> Result<Variant> {
        if dispid == ribbon::dispid::DEFAULT {
            // A host that asks a dispatch object for its default member is given the add-in object
            // itself, which is how an automation client can reach the commands.
            return Ok(Variant::dispatch(&self_dispatch(self)));
        }
        if dispid == ribbon::dispid::GET_CUSTOM_UI {
            return Ok(Variant::bstr(ribbon::CUSTOM_UI));
        }
        if let Some(command) = Command::from_dispid(dispid) {
            return Ok(Variant::bstr(&self.callback(command)));
        }
        match dispid {
            ribbon::dispid::ON_CONNECTION => {
                let application =
                    argument_object(parameters, 0).ok_or_else(|| Error::from(E_POINTER))?;
                let add_in = argument_object(parameters, 2).map(Object::from_dispatch);
                let _ = self.connect(
                    Object::from_dispatch(application),
                    add_in,
                    self_dispatch(self),
                );
                Ok(Variant::empty())
            }
            ribbon::dispid::ON_DISCONNECTION => {
                self.disconnect();
                Ok(Variant::empty())
            }
            ribbon::dispid::ON_ADD_INS_UPDATE
            | ribbon::dispid::ON_STARTUP_COMPLETE
            | ribbon::dispid::ON_BEGIN_SHUTDOWN => Ok(Variant::empty()),
            _ => Err(Error::from(DISP_E_MEMBERNOTFOUND)),
        }
    }
}

impl IDTExtensibility2_Impl for Addin_Impl {
    unsafe fn OnConnection(
        &self,
        application: *mut c_void,
        _connect_mode: i32,
        add_in_instance: *mut c_void,
        _custom: *mut c_void,
    ) -> HRESULT {
        // SAFETY: the host passes its application object and its own add-in object, both of which
        // stay alive while we are connected.
        let Some(application) = (unsafe { Object::from_pointer(application) }) else {
            return E_POINTER;
        };
        let add_in = unsafe { Object::from_pointer(add_in_instance) };
        self.connect(application, add_in, self_dispatch(self))
    }

    unsafe fn OnDisconnection(&self, _remove_mode: i32, _custom: *mut c_void) -> HRESULT {
        self.disconnect();
        S_OK
    }

    unsafe fn OnAddInsUpdate(&self, _custom: *mut c_void) -> HRESULT {
        S_OK
    }

    unsafe fn OnStartupComplete(&self, _custom: *mut c_void) -> HRESULT {
        S_OK
    }

    unsafe fn OnBeginShutdown(&self, _custom: *mut c_void) -> HRESULT {
        S_OK
    }
}

impl IRibbonExtensibility_Impl for Addin_Impl {
    unsafe fn GetCustomUI(&self, _ribbon_id: BSTR, ribbon_xml: *mut BSTR) -> HRESULT {
        if ribbon_xml.is_null() {
            return E_POINTER;
        }
        // SAFETY: the host offers a slot for the markup, and it takes over the string.
        unsafe { ribbon_xml.write(BSTR::from(ribbon::CUSTOM_UI)) };
        S_OK
    }
}

impl IDispatch_Impl for Addin_Impl {
    fn GetTypeInfoCount(&self) -> Result<u32> {
        // No type information is offered: the host uses the member names from the ribbon markup.
        Ok(0)
    }

    fn GetTypeInfo(&self, _type_info: u32, _lcid: u32) -> Result<ITypeInfo> {
        Err(Error::from(E_NOTIMPL))
    }

    fn GetIDsOfNames(
        &self,
        _riid: *const GUID,
        names: *const PCWSTR,
        count: u32,
        _lcid: u32,
        ids: *mut i32,
    ) -> Result<()> {
        if names.is_null() || ids.is_null() {
            return Err(Error::from(E_POINTER));
        }
        for index in 0..count as usize {
            // SAFETY: the host supplies `count` names, each a NUL terminated wide string.
            let name = unsafe { names.add(index).read().to_string()? };
            let id = ribbon::dispid::fixed(&name)
                .or_else(|| Command::from_name(&name).map(Command::dispid))
                .ok_or_else(com::unknown_name)?;
            // SAFETY: the host supplies `count` slots for the answers.
            unsafe { ids.add(index).write(id) };
        }
        Ok(())
    }

    fn Invoke(
        &self,
        dispid: i32,
        _riid: *const GUID,
        _lcid: u32,
        _flags: DISPATCH_FLAGS,
        parameters: *const DISPPARAMS,
        result: *mut VARIANT,
        _exception: *mut EXCEPINFO,
        _argument_error: *mut u32,
    ) -> Result<()> {
        let value = self.dispatch_invoke(dispid, parameters)?;
        if !result.is_null() {
            // SAFETY: the host offers a slot for the result and takes over what is written there.
            unsafe { result.write(value.into_inner()) };
        }
        Ok(())
    }
}

/// Creates add-in objects for the host.
#[implement(IClassFactory)]
struct Factory;

impl Drop for Factory {
    fn drop(&mut self) {
        LIVE_OBJECTS.fetch_sub(1, Ordering::SeqCst);
    }
}

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<'_, IUnknown>,
        interface: *const GUID,
        object: *mut *mut c_void,
    ) -> Result<()> {
        if !outer.is_null() {
            return Err(Error::from(CLASS_E_NOAGGREGATION));
        }
        if object.is_null() {
            return Err(Error::from(E_POINTER));
        }
        // SAFETY: the host offers a slot for the interface it asked for.
        unsafe { *object = std::ptr::null_mut() };
        let addin: IDispatch = Addin {
            connection: Mutex::new(None),
        }
        .into();
        LIVE_OBJECTS.fetch_add(1, Ordering::SeqCst);
        // SAFETY: the query writes into the slot the host supplied.
        unsafe { addin.query(interface, object) }.ok()
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        // The live object count already keeps the server in memory; nothing else is held.
        Ok(())
    }
}

/// Creates the class factory for [`CLSID_ADDIN`].
///
/// # Safety
///
/// Called by COM with a valid class id, a valid interface id and a writable out pointer.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    class: *const GUID,
    interface: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    if class.is_null() || interface.is_null() || object.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller supplied a writable out pointer.
    unsafe { *object = std::ptr::null_mut() };
    // SAFETY: the class id was checked for null above.
    if unsafe { *class } != CLSID_ADDIN {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = Factory.into();
    LIVE_OBJECTS.fetch_add(1, Ordering::SeqCst);
    // SAFETY: the factory is alive for the call, and the query writes into the caller's out slot.
    unsafe { factory.query(interface, object) }
}

/// Reports whether the server may be unloaded, which is once it holds no objects and no factories.
///
/// A factory the host still points at counts, because unloading the library would leave that
/// pointer dangling.
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if LIVE_OBJECTS.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}
