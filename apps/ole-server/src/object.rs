//! The embedded equation object: everything a document host calls.
//!
//! The object holds one equation, three ways: as a tree ([`mt_core::Math`], which is what gets laid
//! out and drawn), as LaTeX text (which is what the editor reads and writes, and what re-opening
//! the object trusts first) and, when neither of those can be read back, as the raw bytes of the
//! stream the document held, so that saving cannot destroy something that was never understood.
//!
//! Activating the object (`OLEIVERB_PRIMARY`, `OLEIVERB_SHOW` or `OLEIVERB_OPEN` — hosts differ in
//! which one a double-click sends, so all three work the same way) hands the LaTeX to the desktop
//! editor and returns at once. The result is picked up by [`EquationObject::tick`], which the
//! message loop calls on a timer: the whole object lives in one apartment, so the storage the host
//! gave us is only ever touched from that thread.

use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use mt_core::Math;
use windows::Win32::Foundation::{
    CO_E_OBJNOTCONNECTED, DISP_E_BADPARAMCOUNT, DISP_E_MEMBERNOTFOUND, DISP_E_UNKNOWNNAME,
    DV_E_DVASPECT, DV_E_FORMATETC, DV_E_TYMED, E_FAIL, E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY,
    E_POINTER, GlobalFree, HGLOBAL, HWND, OLE_E_ADVISENOTSUPPORTED, OLE_E_NOCONNECTION,
    OLE_E_NOTRUNNING, OLE_E_OLEVERB, RECTL, S_FALSE, SIZE,
};
use windows::Win32::Graphics::Gdi::{
    BITMAPINFO, DIB_RGB_COLORS, GetDeviceCaps, HDC, LOGPALETTE, LOGPIXELSX, LOGPIXELSY, SRCCOPY,
    SetEnhMetaFileBits, StretchDIBits,
};
use windows::Win32::System::Com::StructuredStorage::{
    IPersistStorage, IPersistStorage_Impl, IStorage,
};
use windows::Win32::System::Com::{
    DISPATCH_FLAGS, DISPATCH_METHOD, DISPATCH_PROPERTYGET, DISPATCH_PROPERTYPUT,
    DISPATCH_PROPERTYPUTREF, DISPPARAMS, DVASPECT, DVASPECT_CONTENT, DVTARGETDEVICE, EXCEPINFO,
    FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl, IDispatch, IDispatch_Impl,
    IEnumFORMATETC, IEnumSTATDATA, IMoniker, IPersist_Impl, ITypeInfo, STGMEDIUM, STGMEDIUM_0,
    TYMED_ENHMF, TYMED_HGLOBAL, TYMED_MFPICT,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{
    ACTIVEOBJECT_WEAK, CF_ENHMETAFILE, CF_METAFILEPICT, IEnumOLEVERB, IOleClientSite, IOleObject,
    IOleObject_Impl, IViewObject_Impl, IViewObject2, IViewObject2_Impl, OLECLOSE,
    OLECLOSE_SAVEIFDIRTY, OLEGETMONIKER, OLEIVERB_HIDE, OLEIVERB_OPEN, OLEIVERB_PRIMARY,
    OLEIVERB_SHOW, OLEMISC, OLEMISC_RECOMPOSEONRESIZE, OLEMISC_RENDERINGISDEVICEINDEPENDENT,
    OLEWHICHMK, RegisterActiveObject, RevokeActiveObject, USERCLASSTYPE, USERCLASSTYPE_APPNAME,
    USERCLASSTYPE_SHORT,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{
    BOOL, Error, GUID, HRESULT, PCWSTR, PWSTR, Ref, Result as WinResult, implement,
};

use crate::clsid::{self, CLSID_EQUATION, FRIENDLY_NAME, PROGID, STREAM_COMPOBJ, STREAM_LATEX, STREAM_NATIVE};
use crate::factory::ServerState;
use crate::formats::{self, FormatEnumerator};
use crate::handoff::Session;
use crate::picture;
use crate::storage;
use crate::trace;
use crate::variant;

/// The equation a new object starts with.
const DEFAULT_LATEX: &str = "y = x^2";

/// What the object says about itself to a host that asks.
///
/// It is recomposed when its size changes, and its bitmap is the same on every device, which is
/// what those two flags mean.
const MISC_STATUS: i32 = OLEMISC_RECOMPOSEONRESIZE.0 | OLEMISC_RENDERINGISDEVICEINDEPENDENT.0;

/// Automation members. Exposed so the object's behaviour can be checked without a human clicking.
const DISPID_GET_LATEX: i32 = 1;
const DISPID_SET_LATEX: i32 = 2;
const DISPID_SAVE: i32 = 3;
const DISPID_GET_SIZE: i32 = 4;
const DISPID_GET_STATUS: i32 = 5;

/// The equation object a host embeds.
#[implement(
    IOleObject,
    IDataObject,
    IPersistStorage,
    IViewObject2,
    IDispatch,
    Agile = false
)]
pub struct EquationObject {
    state: Arc<ServerState>,
    inner: Rc<RefCell<Inner>>,
}

impl Drop for EquationObject {
    fn drop(&mut self) {
        self.state.object_released();
        trace::log(&format!(
            "released an equation object ({} still with hosts)",
            self.state.live()
        ));
    }
}

/// The running-object-table entry that makes the object reachable by name.
static ACTIVE_COOKIE: AtomicU32 = AtomicU32::new(0);

/// Put the object in the running object table, so tooling can reach it with `GetActiveObject`
/// rather than having to hold on to the interface the host was given.
///
/// Only the first object is registered: it is the one an automation client asking for
/// `MathTypeRs.Equation.1` means.
pub fn register_active(unknown: &windows::core::IUnknown) {
    if ACTIVE_COOKIE.load(Ordering::Relaxed) != 0 {
        return;
    }
    let mut cookie = 0u32;
    match unsafe { RegisterActiveObject(unknown, &CLSID_EQUATION, ACTIVEOBJECT_WEAK, &mut cookie) }
    {
        Ok(()) => {
            ACTIVE_COOKIE.store(cookie, Ordering::Relaxed);
            trace::log(&format!(
                "registered as the active object (cookie {cookie})"
            ));
        }
        Err(error) => trace::log(&format!(
            "cannot register as the active object: {}",
            storage::error_text(&error)
        )),
    }
}

/// Take the object back out of the running object table.
pub fn revoke_active() {
    let cookie = ACTIVE_COOKIE.swap(0, Ordering::Relaxed);
    if cookie != 0 {
        unsafe {
            let _ = RevokeActiveObject(cookie, None);
        }
    }
}

thread_local! {
    /// Every object this server has handed out, so the message loop's timer can drive them.
    ///
    /// The object lives in one apartment, and so does this list; nothing here crosses a thread.
    static LIVE_OBJECTS: RefCell<Vec<std::rc::Weak<RefCell<Inner>>>> = const { RefCell::new(Vec::new()) };
}

/// Read back whatever every live object's editor session has left behind.
pub fn tick_all() {
    LIVE_OBJECTS.with(|live| {
        let mut live = live.borrow_mut();
        live.retain(|weak| match weak.upgrade() {
            Some(inner) => {
                tick(&inner);
                true
            }
            None => false,
        });
    });
}

/// The equation, and everything about it that is not a COM pointer of the host's.
struct Inner {
    /// The equation as a tree, laid out and drawn from here.
    math: Math,
    /// The equation as LaTeX text: what the editor reads and writes.
    latex: String,
    /// The equation stream exactly as it was loaded, kept when it could not be read.
    raw_native: Option<Vec<u8>>,
    /// The equation preferences the loaded stream carried, so a save writes them back.
    preferences: Option<mt_mtef::Preferences>,
    /// Full size of the equation in points.
    size_points: f64,
    /// Whether the LaTeX text is something the parser could not read.
    unparsed: bool,
    dirty: bool,
    closed: bool,
    /// What the host last asked the object to be, in HIMETRIC units.
    extent: Option<(i32, i32)>,
    /// Storage the host handed over, and the site and sinks to tell about changes.
    storage: Option<IStorage>,
    client_site: Option<IOleClientSite>,
    advises: Vec<(u32, IAdviseSink)>,
    view_advise: Option<(u32, IAdviseSink)>,
    next_advise: u32,
    /// The editor running on this object, if it is.
    session: Option<Session>,
    /// The last thing worth reporting about the object.
    status: String,
}

impl Inner {
    fn new() -> Self {
        let (math, latex) = default_equation();
        Self {
            math,
            latex,
            raw_native: None,
            preferences: None,
            size_points: picture::DEFAULT_SIZE_POINTS,
            unparsed: false,
            dirty: false,
            closed: false,
            extent: None,
            storage: None,
            client_site: None,
            advises: Vec::new(),
            view_advise: None,
            next_advise: 1,
            session: None,
            status: "new object".to_owned(),
        }
    }

    /// Take the equation back from the two streams a document held.
    ///
    /// The LaTeX stream is trusted first, because it is the one the editor reads and writes. When
    /// it is missing or does not parse, the equation stream stands in for it; when that does not
    /// parse either, its bytes are kept exactly as they are, so the next save writes back what was
    /// loaded rather than an empty equation.
    fn restore(&mut self, latex: Option<Vec<u8>>, native: Option<Vec<u8>>) {
        self.raw_native = None;
        // Keep the preferences the equation stream was written with, so that saving writes the
        // object back the way the document held it rather than with our defaults.
        self.preferences = native
            .as_ref()
            .and_then(|bytes| mt_mtef::read(bytes).ok())
            .map(|parsed| parsed.header.preferences);
        let text = latex.map(|raw| storage::text_of(&raw));
        if let Some(text) = &text {
            self.latex = text.clone();
            match mt_tex::parse(text) {
                Ok(math) => {
                    self.math = math;
                    self.unparsed = false;
                    return;
                }
                Err(error) => {
                    self.unparsed = true;
                    trace::log(&format!(
                        "the stored equation text does not parse (byte {}: {}); keeping it as it stands",
                        error.offset, error.message
                    ));
                }
            }
        }
        if let Some(bytes) = native {
            match mt_mtef::parse(&bytes) {
                Ok(math) => {
                    trace::log("taking the equation from the equation stream");
                    self.math = math;
                    if text.is_none() {
                        self.latex = mt_tex::to_latex(&self.math);
                        self.unparsed = false;
                    }
                    return;
                }
                Err(error) => {
                    trace::log(&format!(
                        "the equation stream does not parse ({error}); keeping its bytes"
                    ));
                    self.raw_native = Some(bytes);
                    return;
                }
            }
        }
        if text.is_none() {
            trace::log("the document held no equation; starting from the default one");
        }
    }

    /// Take `text` as the object's equation, keeping it even when it does not parse.
    ///
    /// Returns whether it parsed.
    fn replace_equation(&mut self, text: &str) -> bool {
        self.latex = storage::text_of(text.as_bytes());
        self.raw_native = None;
        match mt_tex::parse(&self.latex) {
            Ok(math) => {
                self.math = math;
                self.unparsed = false;
                true
            }
            Err(error) => {
                self.unparsed = true;
                trace::log(&format!(
                    "the new text does not parse (byte {}: {}); keeping it as it stands",
                    error.offset, error.message
                ));
                false
            }
        }
    }

    fn note(&mut self, message: &str) {
        self.status = message.to_owned();
    }

    fn sinks(&self) -> Vec<(u32, IAdviseSink)> {
        self.advises.clone()
    }
}

impl EquationObject {
    /// A new object, counted in `state` until it is released.
    pub fn new(state: Arc<ServerState>) -> Self {
        state.object_created();
        let inner = Rc::new(RefCell::new(Inner::new()));
        LIVE_OBJECTS.with(|live| live.borrow_mut().push(Rc::downgrade(&inner)));
        trace::log("creating an equation object");
        Self { state, inner }
    }

    /// Start the editor on the equation, unless one is already running on it.
    fn open_editor(&self) -> WinResult<()> {
        self.state.note_busy();
        let mut inner = self.inner.borrow_mut();
        let already_open = match inner.session.as_mut() {
            Some(session) if session.running() => Some(session.editor_pid),
            _ => None,
        };
        if let Some(pid) = already_open {
            inner.note(&format!("the editor is already open (pid {pid})"));
            drop(inner);
            trace::log("activation while the editor is already open");
            return Ok(());
        }
        let latex = inner.latex.clone();
        match Session::start(&latex) {
            Ok(session) => {
                trace::log(&format!(
                    "the editor is running (pid {})",
                    session.editor_pid
                ));
                inner.note(&format!(
                    "editor pid {} on {}",
                    session.editor_pid,
                    session.path.display()
                ));
                inner.session = Some(session);
                Ok(())
            }
            Err(message) => {
                inner.note(&message);
                drop(inner);
                trace::log(&format!("cannot start the editor: {message}"));
                Err(Error::new(E_FAIL, message))
            }
        }
    }
}

/// Read back a finished editor session and tell the host about it.
///
/// Called from the message loop's timer; it does nothing while the editor is still running.
fn tick(inner: &RefCell<Inner>) {
    let finished = {
        let mut state = inner.borrow_mut();
        match state.session.as_mut() {
            Some(session) => session.poll(),
            None => return,
        }
    };
    let Some(outcome) = finished else {
        return;
    };

    let mut changed = false;
    let mut message = String::new();
    {
        let mut state = inner.borrow_mut();
        if let Some(session) = state.session.take() {
            message = match outcome {
                Ok(text) if text == state.latex => {
                    format!(
                        "session {} closed without changing the equation",
                        session.id
                    )
                }
                Ok(text) => {
                    changed = true;
                    let length = text.len();
                    let parsed = state.replace_equation(&text);
                    format!(
                        "session {} wrote {length} bytes{}",
                        session.id,
                        if parsed {
                            ""
                        } else {
                            " (not parseable, kept as text)"
                        }
                    )
                }
                Err(error) => error,
            };
        }
        state.note(&message);
        if changed {
            state.dirty = true;
        }
    }
    trace::log(&message);
    if !changed {
        return;
    }

    match write_streams(inner) {
        Ok(note) => {
            trace::log(&note);
            // The streams are in the host's storage, but the host has not saved the document that
            // holds them, so the object stays dirty: `IsDirty` is where the host asks whether its
            // document has changes, and that is what makes it save. The host is deliberately not
            // asked to save from here, because it may be inside a save of its own at this very
            // moment: a container told to save while it is saving does not come out of it.
            inner.borrow_mut().note(&note);
        }
        Err(error) => {
            trace::log(&format!("the new equation is held in memory only: {error}"));
            inner.borrow_mut().note(&error);
        }
    }
}

/// Whether closing writes the streams first: only `OLECLOSE_SAVEIFDIRTY` on a dirty object saves.
fn save_on_close(option: OLECLOSE, dirty: bool) -> bool {
    option.0 == OLECLOSE_SAVEIFDIRTY.0 && dirty
}

/// Whether the verb activates the editor: the standard activation verbs, plus every positive
/// verb index a host read out of the registry, all of which mean `&Edit` for this object.
fn opens_editor(iverb: i32) -> bool {
    iverb == OLEIVERB_PRIMARY.0 || iverb == OLEIVERB_SHOW.0 || iverb == OLEIVERB_OPEN.0 || iverb > 0
}

/// Write both streams into the host's storage and commit them.
fn write_streams(inner: &RefCell<Inner>) -> Result<String, String> {
    let (storage, native, latex) = {
        let state = inner.borrow();
        let Some(storage) = state.storage.clone() else {
            return Err("the object has no storage, so the equation stays in memory".to_owned());
        };
        // Write the equation back with the preferences the loaded stream carried, so an
        // embedded object round-trips the document's typesizes, spacings and styles; a stream
        // this server could not read keeps its raw bytes, and anything else gets the defaults.
        let native = state
            .raw_native
            .clone()
            .unwrap_or_else(|| match &state.preferences {
                Some(preferences) => mt_mtef::write_with_preferences(&state.math, preferences),
                None => mt_mtef::write(&state.math),
            });
        (storage, native, state.latex.clone())
    };
    // The storage says whose it is and carries the class information stream, which is how a
    // host names the object and finds its way back to this server when the document is opened
    // again; the equation streams alone leave it reporting "Unknown" and no programmatic
    // identifier.
    storage::set_class(&storage, &CLSID_EQUATION)?;
    storage::write_stream(&storage, STREAM_COMPOBJ, &clsid::compobj())?;
    storage::write_stream(&storage, STREAM_NATIVE, &native)?;
    storage::write_stream(&storage, STREAM_LATEX, latex.as_bytes())?;
    storage::commit(&storage)?;
    Ok(format!(
        "wrote {STREAM_COMPOBJ}, {STREAM_NATIVE} ({} bytes) and {STREAM_LATEX} ({} bytes)",
        native.len(),
        latex.len()
    ))
}

/// Tell the host's advise sinks that the object was saved and that its picture may have changed.
///
/// This is what the sinks are for, and it is deliberately all that is said to the host from here:
/// asking the host to save its document is not this object's business at this point, because the
/// object being dirty already says so where the host looks for it (`IsDirty`), and a container
/// that is told to save while it is saving is a container that stops.
fn announce_saved(inner: &RefCell<Inner>) {
    let (sinks, view) = {
        let state = inner.borrow();
        (state.sinks(), state.view_advise.clone())
    };
    for (connection, sink) in sinks {
        trace::log(&format!("advise connection {connection}: OnSave"));
        crate::notify::on_save(&sink);
    }
    if let Some((aspects, sink)) = view {
        trace::log("advise connection: OnViewChange");
        crate::notify::on_view_change(&sink, aspects);
    }
}

impl EquationObject {
    /// The equation's size in `HIMETRIC` units, as the host last asked for it or as it lays out.
    fn measurement(&self) -> (i32, i32) {
        let inner = self.inner.borrow();
        match inner.extent {
            Some((cx, cy)) => (cx, cy),
            None => picture::himetric_size(&inner.math, inner.size_points),
        }
    }

    /// The equation's own size in points.
    fn size_in_points(&self) -> (f64, f64) {
        let inner = self.inner.borrow();
        picture::natural_size(&inner.math, inner.size_points)
    }

    /// The bitmap of the equation, or why there is none.
    fn picture(&self) -> Result<picture::Presentation, Error> {
        let (math, extent, size_points) = {
            let inner = self.inner.borrow();
            (inner.math.clone(), inner.extent, inner.size_points)
        };
        picture::presentation(&math, size_points, extent)
            .ok_or_else(|| Error::new(E_FAIL, "the equation could not be laid out"))
    }

    /// Replace the equation from automation, reporting an error string or nothing.
    fn set_latex(&self, text: &str) -> String {
        self.state.note_busy();
        let mut inner = self.inner.borrow_mut();
        let parsed = inner.replace_equation(text);
        inner.dirty = true;
        inner.note(&format!("SetLatex: {} bytes of text", text.len()));
        drop(inner);
        trace::log(&format!(
            "SetLatex: {} bytes of text, parsed={parsed}",
            text.len()
        ));
        if parsed {
            String::new()
        } else {
            "the text was kept but does not parse as LaTeX".to_owned()
        }
    }

    /// Write the streams on demand, the way the automation `Save` member does.
    ///
    /// The object stays dirty afterwards: its streams are in the host's storage, but the host has
    /// not saved the document that holds them, and `IsDirty` is what asks it to.
    fn save_now(&self) -> String {
        self.state.note_busy();
        match write_streams(&self.inner) {
            Ok(note) => {
                self.inner.borrow_mut().note(&note);
                announce_saved(&self.inner);
                String::new()
            }
            Err(error) => error,
        }
    }

    /// One line per fact about the object, for a report.
    fn status_text(&self) -> String {
        let inner = self.inner.borrow();
        let (width, height) = picture::natural_size(&inner.math, inner.size_points);
        let session = match &inner.session {
            Some(session) => format!(
                "editor pid {} on {}",
                session.editor_pid,
                session.path.display()
            ),
            None => "no editor open".to_owned(),
        };
        let extent = match inner.extent {
            Some((cx, cy)) => format!("{cx}x{cy} HIMETRIC"),
            None => "as laid out".to_owned(),
        };
        let streams = if inner.storage.is_some() {
            "with storage"
        } else {
            "without storage"
        };
        [
            format!("latex={}", inner.latex),
            format!("size={width:.2}x{height:.2} points"),
            format!("extent={extent}"),
            streams.to_owned(),
            format!(
                "dirty={} closed={} unparsed={}",
                inner.dirty, inner.closed, inner.unparsed
            ),
            format!("session={session}"),
            format!("last={}", inner.status),
        ]
        .join("; ")
    }
}

impl IOleObject_Impl for EquationObject_Impl {
    fn SetClientSite(&self, pclientsite: Ref<IOleClientSite>) -> WinResult<()> {
        let mut inner = self.inner.borrow_mut();
        inner.client_site = pclientsite.cloned();
        inner.note("the host handed over its client site");
        drop(inner);
        self.state.note_busy();
        trace::log("SetClientSite");
        Ok(())
    }

    fn GetClientSite(&self) -> WinResult<IOleClientSite> {
        trace::log("call GetClientSite");
        self.inner
            .borrow()
            .client_site
            .clone()
            .ok_or_else(|| Error::new(CO_E_OBJNOTCONNECTED, "the object has no client site"))
    }

    fn SetHostNames(&self, szcontainerapp: &PCWSTR, szcontainerobj: &PCWSTR) -> WinResult<()> {
        // The names the host uses for this object are for its own window titles, which the object
        // does not draw; they are recorded because they say who is hosting us.
        let app = unsafe { szcontainerapp.to_string() }.unwrap_or_default();
        let object = unsafe { szcontainerobj.to_string() }.unwrap_or_default();
        trace::log(&format!("SetHostNames({app:?}, {object:?})"));
        Ok(())
    }

    fn Close(&self, dwsaveoption: &OLECLOSE) -> WinResult<()> {
        self.state.note_busy();
        let mut inner = self.inner.borrow_mut();
        if inner.closed {
            return Ok(());
        }
        if let Some(session) = inner.session.as_mut() {
            session.forget();
        }
        inner.session = None;
        // A dirty object closed with `OLECLOSE_SAVEIFDIRTY` must have its streams written before
        // the storage is dropped, or the document the host then saves holds an old equation and
        // the changes are gone. The save is the same one the automation `Save` member does.
        if !save_on_close(*dwsaveoption, inner.dirty) {
            inner.closed = true;
            inner.storage = None;
            inner.note("closed by the host");
            drop(inner);
            trace::log(&format!("Close(save={})", dwsaveoption.0));
            self.state.object_closed();
            return Ok(());
        }
        drop(inner);
        match write_streams(&self.inner) {
            Ok(note) => {
                {
                    let mut inner = self.inner.borrow_mut();
                    inner.dirty = false;
                    inner.closed = true;
                    inner.storage = None;
                    inner.note(&note);
                }
                trace::log(&format!("Close(save={}): {note}", dwsaveoption.0));
                announce_saved(&self.inner);
                self.state.object_closed();
                Ok(())
            }
            Err(error) => {
                // The storage is kept and the object stays open: the host can retry the save it
                // asked for, and the error tells it why closing did not take.
                trace::log(&format!(
                    "Close(save={}): refusing to drop a dirty object: {error}",
                    dwsaveoption.0
                ));
                self.inner
                    .borrow_mut()
                    .note(&format!("closing without saving failed: {error}"));
                Err(Error::new(
                    E_FAIL,
                    format!("the object is dirty and its streams could not be written: {error}"),
                ))
            }
        }
    }

    fn SetMoniker(&self, _dwwhichmoniker: &OLEWHICHMK, _pmk: Ref<IMoniker>) -> WinResult<()> {
        // Embedded objects have no moniker of their own; the host owns the document one.
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn GetMoniker(
        &self,
        _dwassign: &OLEGETMONIKER,
        _dwwhichmoniker: &OLEWHICHMK,
    ) -> WinResult<IMoniker> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn InitFromData(
        &self,
        _pdataobject: Ref<windows::Win32::System::Com::IDataObject>,
        _fcreation: BOOL,
        _dwreserved: u32,
    ) -> WinResult<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn GetClipboardData(
        &self,
        _dwreserved: u32,
    ) -> WinResult<windows::Win32::System::Com::IDataObject> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn DoVerb(
        &self,
        iverb: i32,
        _lpmsg: *const windows::Win32::UI::WindowsAndMessaging::MSG,
        _pactivesite: Ref<IOleClientSite>,
        _lindex: i32,
        _hwndparent: HWND,
        _lprcposrect: *const windows::Win32::Foundation::RECT,
    ) -> WinResult<()> {
        // A host's double-click is `OLEIVERB_PRIMARY`, its "Open" is `OLEIVERB_OPEN`, and an
        // object that shows itself on its own is asked to `OLEIVERB_SHOW`; Word and WPS 文字 do
        // not agree on which one they send, so all three open the editor. Hosts also call the
        // verbs the registry lists by index — install.ps1 registers `Verb\1` as `&Edit` — and
        // every verb this object advertises opens the editor, so any positive index does too.
        let open = opens_editor(iverb);
        trace::log(&format!(
            "DoVerb({iverb}) -> {}",
            if open {
                "open the editor"
            } else {
                "unsupported"
            }
        ));
        if open {
            return self.open_editor();
        }
        if iverb == OLEIVERB_HIDE.0 {
            return Ok(());
        }
        Err(Error::from_hresult(OLE_E_OLEVERB))
    }

    fn EnumVerbs(&self) -> WinResult<IEnumOLEVERB> {
        // The verbs this object has are the ones the host looks up for itself: opening it, which
        // is what every activation does.
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn Update(&self) -> WinResult<()> {
        trace::log("call Update");
        Ok(())
    }

    fn IsUpToDate(&self) -> WinResult<()> {
        trace::log("call IsUpToDate");
        Ok(())
    }

    fn GetUserClassID(&self) -> WinResult<GUID> {
        trace::log("call GetUserClassID");
        Ok(CLSID_EQUATION)
    }

    fn GetUserType(&self, dwformoftype: &USERCLASSTYPE) -> WinResult<PWSTR> {
        trace::log("call GetUserType");
        let text = if *dwformoftype == USERCLASSTYPE_SHORT {
            "Equation"
        } else if *dwformoftype == USERCLASSTYPE_APPNAME {
            PROGID
        } else {
            FRIENDLY_NAME
        };
        Ok(task_memory_string(text))
    }

    fn SetExtent(&self, dwdrawaspect: DVASPECT, psizel: *const SIZE) -> WinResult<()> {
        trace::log("call SetExtent");
        self.state.note_busy();
        if dwdrawaspect != DVASPECT_CONTENT || psizel.is_null() {
            return Ok(());
        }
        let size = unsafe { *psizel };
        let mut inner = self.inner.borrow_mut();
        inner.extent = if size.cx > 0 && size.cy > 0 {
            Some((size.cx, size.cy))
        } else {
            None
        };
        inner.note(&format!(
            "the host sized the object to {}x{} HIMETRIC",
            size.cx, size.cy
        ));
        drop(inner);
        trace::log(&format!("SetExtent({}x{} HIMETRIC)", size.cx, size.cy));
        Ok(())
    }

    fn GetExtent(&self, dwdrawaspect: DVASPECT) -> WinResult<SIZE> {
        trace::log("call GetExtent");
        if dwdrawaspect != DVASPECT_CONTENT {
            return Err(Error::from_hresult(DV_E_DVASPECT));
        }
        let (cx, cy) = self.measurement();
        trace::log(&format!("GetExtent -> {cx}x{cy} HIMETRIC"));
        Ok(SIZE { cx, cy })
    }

    fn Advise(&self, padvsink: Ref<IAdviseSink>) -> WinResult<u32> {
        let Some(sink) = padvsink.cloned() else {
            return Err(Error::from_hresult(E_POINTER));
        };
        let mut inner = self.inner.borrow_mut();
        let connection = inner.next_advise;
        inner.next_advise += 1;
        inner.advises.push((connection, sink));
        trace::log(&format!("Advise -> connection {connection}"));
        Ok(connection)
    }

    fn Unadvise(&self, dwconnection: u32) -> WinResult<()> {
        let mut inner = self.inner.borrow_mut();
        let before = inner.advises.len();
        inner
            .advises
            .retain(|(connection, _)| *connection != dwconnection);
        if inner.advises.len() == before {
            return Err(Error::from_hresult(OLE_E_NOCONNECTION));
        }
        trace::log(&format!("Unadvise({dwconnection})"));
        Ok(())
    }

    fn EnumAdvise(&self) -> WinResult<IEnumSTATDATA> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn GetMiscStatus(&self, dwaspect: DVASPECT) -> WinResult<OLEMISC> {
        trace::log("call GetMiscStatus");
        if dwaspect != DVASPECT_CONTENT {
            return Ok(OLEMISC(0));
        }
        Ok(OLEMISC(MISC_STATUS))
    }

    fn SetColorScheme(&self, _plogpal: *const LOGPALETTE) -> WinResult<()> {
        // The equation is drawn in one colour, whatever palette the host is using.
        Err(Error::from_hresult(E_NOTIMPL))
    }
}

impl IDataObject_Impl for EquationObject_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> WinResult<STGMEDIUM> {
        trace::log("call GetData");
        if pformatetcin.is_null() {
            return Err(Error::from_hresult(DV_E_FORMATETC));
        }
        let format = unsafe { *pformatetcin };
        if !formats::supported(&format) {
            let reason = formats::rejection(&format);
            trace::log(&format!(
                "GetData(cf={}, aspect={}, tymed={}, lindex={}) -> refused {:#010X}",
                format.cfFormat, format.dwAspect, format.tymed, format.lindex, reason.0 as u32
            ));
            return Err(Error::from_hresult(reason));
        }
        let picture = self.picture()?;
        if format.cfFormat == CF_METAFILEPICT.0 {
            // The classic picture: the same equation as a Windows-format metafile inside a
            // `METAFILEPICT` the host frees with `ReleaseStgMedium`.
            let Some(memory) = picture::metafile_picture(&picture) else {
                trace::log("GetData -> CF_METAFILEPICT could not be built");
                return Err(Error::new(
                    E_FAIL,
                    "the equation metafile picture could not be built",
                ));
            };
            trace::log(&format!(
                "GetData -> CF_METAFILEPICT ({:.1}x{:.1} points)",
                picture.width_pt, picture.height_pt
            ));
            return Ok(STGMEDIUM {
                tymed: TYMED_MFPICT.0 as u32,
                u: STGMEDIUM_0 {
                    hMetaFilePict: memory.0,
                },
                pUnkForRelease: ManuallyDrop::new(None),
            });
        }
        if format.cfFormat == CF_ENHMETAFILE.0 {
            // The vector picture: the metafile bytes become a GDI handle the host plays back at
            // whatever size it is drawing, so zooming stays sharp.
            let handle = unsafe { SetEnhMetaFileBits(&picture.emf) };
            if handle.is_invalid() {
                return Err(Error::new(
                    E_FAIL,
                    "the equation metafile could not be created",
                ));
            }
            trace::log(&format!(
                "GetData -> CF_ENHMETAFILE ({:.1}x{:.1} points), {} bytes",
                picture.width_pt,
                picture.height_pt,
                picture.emf.len()
            ));
            return Ok(STGMEDIUM {
                tymed: TYMED_ENHMF.0 as u32,
                u: STGMEDIUM_0 {
                    hEnhMetaFile: handle,
                },
                pUnkForRelease: ManuallyDrop::new(None),
            });
        }
        let bytes = &picture.dib;
        unsafe {
            let handle: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
            let target = GlobalLock(handle);
            if target.is_null() {
                let _ = GlobalFree(Some(handle));
                return Err(Error::from_hresult(E_OUTOFMEMORY));
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len());
            let _ = GlobalUnlock(handle);
            trace::log(&format!(
                "GetData -> CF_DIB {}x{} pixels ({:.1}x{:.1} points), {} bytes",
                picture.pixels.0,
                picture.pixels.1,
                picture.width_pt,
                picture.height_pt,
                bytes.len()
            ));
            Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 { hGlobal: handle },
                pUnkForRelease: ManuallyDrop::new(None),
            })
        }
    }

    fn GetDataHere(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *mut STGMEDIUM,
    ) -> WinResult<()> {
        Err(Error::from_hresult(DV_E_TYMED))
    }

    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        trace::log("call QueryGetData");
        if pformatetc.is_null() {
            return DV_E_FORMATETC;
        }
        let format = unsafe { *pformatetc };
        if formats::supported(&format) {
            HRESULT(0)
        } else {
            formats::rejection(&format)
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        pformatectin: *const FORMATETC,
        pformatetcout: *mut FORMATETC,
    ) -> HRESULT {
        trace::log("call GetCanonicalFormatEtc");
        if pformatectin.is_null() || pformatetcout.is_null() {
            return E_NOTIMPL;
        }
        let format = unsafe { *pformatectin };
        // The object has one presentation, so what came in is what goes out, without a target
        // device, which is what `DATA_S_SAMEFORMATETC` means.
        unsafe { pformatetcout.write(formats::canonical(&format)) };
        DATA_S_SAMEFORMATETC
    }

    fn SetData(
        &self,
        pformatetc: *const FORMATETC,
        _pmedium: *const STGMEDIUM,
        _frelease: BOOL,
    ) -> WinResult<()> {
        // Nothing a host can push into this object changes the equation, so the data is ignored
        // rather than refused: refusing makes some hosts give up on the object altogether.
        let format = if pformatetc.is_null() {
            0
        } else {
            unsafe { (*pformatetc).cfFormat }
        };
        trace::log(&format!("SetData(cf={format}) ignored"));
        Ok(())
    }

    fn EnumFormatEtc(&self, dwdirection: u32) -> WinResult<IEnumFORMATETC> {
        if formats::unsupported_direction(dwdirection) {
            return Err(Error::from_hresult(E_NOTIMPL));
        }
        trace::log("EnumFormatEtc -> CF_ENHMETAFILE then CF_DIB, DVASPECT_CONTENT");
        Ok(FormatEnumerator::for_get())
    }

    fn DAdvise(
        &self,
        _pformatetc: *const FORMATETC,
        _advf: u32,
        _padvsink: Ref<IAdviseSink>,
    ) -> WinResult<u32> {
        Err(Error::from_hresult(OLE_E_ADVISENOTSUPPORTED))
    }

    fn DUnadvise(&self, _dwconnection: u32) -> WinResult<()> {
        Err(Error::from_hresult(OLE_E_NOCONNECTION))
    }

    fn EnumDAdvise(&self) -> WinResult<IEnumSTATDATA> {
        Err(Error::from_hresult(OLE_E_ADVISENOTSUPPORTED))
    }
}

impl IViewObject_Impl for EquationObject_Impl {
    fn Draw(
        &self,
        dwdrawaspect: DVASPECT,
        _lindex: i32,
        _pvaspect: *mut c_void,
        _ptd: *const DVTARGETDEVICE,
        _hdctargetdev: HDC,
        hdcdraw: HDC,
        lprcbounds: *const RECTL,
        _lprcwbounds: *const RECTL,
        _pfncontinue: isize,
        _dwcontinue: usize,
    ) -> WinResult<()> {
        trace::log("call Draw");
        if dwdrawaspect != DVASPECT_CONTENT || lprcbounds.is_null() || hdcdraw.is_invalid() {
            return Err(Error::from_hresult(DV_E_DVASPECT));
        }
        let picture = self.picture()?;
        let bounds = unsafe { *lprcbounds };
        let (dpi_x, dpi_y) = unsafe {
            (
                GetDeviceCaps(Some(hdcdraw), LOGPIXELSX).max(1),
                GetDeviceCaps(Some(hdcdraw), LOGPIXELSY).max(1),
            )
        };
        // The bounds arrive in HIMETRIC units and the device wants pixels.
        let pixels = |himetric: i32, dpi: i32| {
            (f64::from(himetric) / 2540.0 * f64::from(dpi)).round() as i32
        };
        let (width, height) = (
            pixels(bounds.right - bounds.left, dpi_x),
            pixels(bounds.bottom - bounds.top, dpi_y),
        );
        if width <= 0 || height <= 0 {
            return Ok(());
        }
        let pixels_at = picture::pixel_offset();
        let bits = picture.dib[pixels_at..].as_ptr().cast::<c_void>();
        let info = picture.dib.as_ptr().cast::<BITMAPINFO>();
        let drawn = unsafe {
            StretchDIBits(
                hdcdraw,
                bounds.left,
                bounds.top,
                width,
                height,
                0,
                0,
                picture.pixels.0 as i32,
                picture.pixels.1 as i32,
                Some(bits),
                info,
                DIB_RGB_COLORS,
                SRCCOPY,
            )
        };
        trace::log(&format!(
            "Draw -> {width}x{height} pixels at {},{} ({drawn} scan lines)",
            bounds.left, bounds.top
        ));
        Ok(())
    }

    fn GetColorSet(
        &self,
        _dwdrawaspect: DVASPECT,
        _lindex: i32,
        _pvaspect: *mut c_void,
        _ptd: *const DVTARGETDEVICE,
        _hictargetdev: HDC,
        _ppcolorset: *mut *mut LOGPALETTE,
    ) -> WinResult<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn Freeze(
        &self,
        _dwdrawaspect: DVASPECT,
        _lindex: i32,
        _pvaspect: *mut c_void,
        _pdwfreeze: *mut u32,
    ) -> WinResult<()> {
        Err(Error::from_hresult(OLE_E_NOTRUNNING))
    }

    fn Unfreeze(&self, _dwfreeze: u32) -> WinResult<()> {
        Err(Error::from_hresult(OLE_E_NOTRUNNING))
    }

    fn SetAdvise(
        &self,
        aspects: DVASPECT,
        _advf: u32,
        padvsink: Ref<IAdviseSink>,
    ) -> WinResult<()> {
        trace::log("call SetAdvise");
        let mut inner = self.inner.borrow_mut();
        inner.view_advise = padvsink.cloned().map(|sink| (aspects.0, sink));
        trace::log(&format!("SetAdvise(aspects={:#X})", aspects.0));
        Ok(())
    }

    fn GetAdvise(
        &self,
        _paspects: *mut u32,
        _padvf: *mut u32,
        _ppadvsink: windows::core::OutRef<IAdviseSink>,
    ) -> WinResult<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
}

impl IViewObject2_Impl for EquationObject_Impl {
    fn GetExtent(
        &self,
        dwdrawaspect: DVASPECT,
        _lindex: i32,
        _ptd: *const DVTARGETDEVICE,
    ) -> WinResult<SIZE> {
        trace::log("call GetExtent (IViewObject2)");
        if dwdrawaspect != DVASPECT_CONTENT {
            return Err(Error::from_hresult(DV_E_DVASPECT));
        }
        let (cx, cy) = self.measurement();
        Ok(SIZE { cx, cy })
    }
}

impl IPersist_Impl for EquationObject_Impl {
    fn GetClassID(&self) -> WinResult<GUID> {
        Ok(CLSID_EQUATION)
    }
}

impl IPersistStorage_Impl for EquationObject_Impl {
    fn IsDirty(&self) -> HRESULT {
        trace::log("call IsDirty");
        if self.inner.borrow().dirty {
            HRESULT(0)
        } else {
            S_FALSE
        }
    }

    fn InitNew(&self, pstg: Ref<IStorage>) -> WinResult<()> {
        self.state.note_busy();
        {
            let mut inner = self.inner.borrow_mut();
            inner.storage = pstg.cloned();
            inner.dirty = false;
            inner.note("the host created the object");
        }
        trace::log("InitNew");
        // The document has an equation from the moment the object exists, so a host that saves
        // without ever activating it still stores a whole object.
        match write_streams(&self.inner) {
            Ok(note) => {
                trace::log(&note);
                Ok(())
            }
            Err(error) => Err(Error::new(E_FAIL, error)),
        }
    }

    fn Load(&self, pstg: Ref<IStorage>) -> WinResult<()> {
        self.state.note_busy();
        let storage = pstg.cloned();
        {
            let mut inner = self.inner.borrow_mut();
            inner.storage = storage.clone();
        }
        let (latex, native) = match &storage {
            Some(storage) => (
                storage::read_stream(storage, STREAM_LATEX)
                    .map_err(|error| Error::new(E_FAIL, error))?,
                storage::read_stream(storage, STREAM_NATIVE)
                    .map_err(|error| Error::new(E_FAIL, error))?,
            ),
            None => (None, None),
        };
        let mut inner = self.inner.borrow_mut();
        inner.restore(latex, native);
        inner.dirty = false;
        let (width, height) = picture::natural_size(&inner.math, inner.size_points);
        inner.note(&format!("loaded {width:.1}x{height:.1} points of equation"));
        trace::log(&format!("Load -> {}", inner.status));
        Ok(())
    }

    fn Save(&self, pstgsave: Ref<IStorage>, fsameasload: BOOL) -> WinResult<()> {
        self.state.note_busy();
        if let Some(storage) = pstgsave.cloned() {
            self.inner.borrow_mut().storage = Some(storage);
        }
        trace::log(&format!("Save(same as load: {})", fsameasload.as_bool()));
        match write_streams(&self.inner) {
            Ok(note) => {
                let mut inner = self.inner.borrow_mut();
                inner.dirty = false;
                inner.note(&note);
                trace::log(&note);
                Ok(())
            }
            Err(error) => Err(Error::new(E_FAIL, error)),
        }
    }

    fn SaveCompleted(&self, pstgnew: Ref<IStorage>) -> WinResult<()> {
        if let Some(storage) = pstgnew.cloned() {
            self.inner.borrow_mut().storage = Some(storage);
        }
        trace::log("SaveCompleted");
        Ok(())
    }

    fn HandsOffStorage(&self) -> WinResult<()> {
        // The equation is in memory, so letting go of the storage costs nothing: the host will
        // hand one back through `Save` or `SaveCompleted`.
        let mut inner = self.inner.borrow_mut();
        inner.storage = None;
        inner.note("the host took its storage back");
        drop(inner);
        trace::log("HandsOffStorage");
        Ok(())
    }
}

impl IDispatch_Impl for EquationObject_Impl {
    fn GetTypeInfoCount(&self) -> WinResult<u32> {
        // The object is late bound; there is no type information to hand out.
        Ok(0)
    }

    fn GetTypeInfo(&self, _itinfo: u32, _lcid: u32) -> WinResult<ITypeInfo> {
        Err(Error::from_hresult(E_NOTIMPL))
    }

    fn GetIDsOfNames(
        &self,
        _riid: *const GUID,
        rgsznames: *const PCWSTR,
        cnames: u32,
        _lcid: u32,
        rgdispid: *mut i32,
    ) -> WinResult<()> {
        if rgsznames.is_null() || rgdispid.is_null() {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        for index in 0..cnames as usize {
            let name = unsafe { PCWSTR::from_raw((*rgsznames.add(index)).0) };
            let name = unsafe { name.to_string() }.unwrap_or_default();
            let member = dispid_of(&name);
            if member == DISPID_UNKNOWN {
                trace::log(&format!("GetIDsOfNames({name:?}) -> unknown"));
                return Err(Error::from_hresult(DISP_E_UNKNOWNNAME));
            }
            unsafe { rgdispid.add(index).write(member) };
        }
        Ok(())
    }

    fn Invoke(
        &self,
        dispidmember: i32,
        _riid: *const GUID,
        _lcid: u32,
        wflags: DISPATCH_FLAGS,
        pdispparams: *const DISPPARAMS,
        pvarresult: *mut VARIANT,
        _pexcepinfo: *mut EXCEPINFO,
        _puargerr: *mut u32,
    ) -> WinResult<()> {
        if wflags.contains(DISPATCH_PROPERTYPUT) || wflags.contains(DISPATCH_PROPERTYPUTREF) {
            // Nothing here is assigned to; the members are all called.
            return Err(Error::from_hresult(DISP_E_MEMBERNOTFOUND));
        }
        let reading = wflags.0 == 0
            || wflags.contains(DISPATCH_METHOD)
            || wflags.contains(DISPATCH_PROPERTYGET);
        if !reading {
            return Err(Error::from_hresult(DISP_E_MEMBERNOTFOUND));
        }
        let value = match dispidmember {
            DISPID_GET_LATEX => variant::text(&self.inner.borrow().latex),
            DISPID_GET_SIZE => {
                let (width, height) = self.size_in_points();
                variant::numbers(&[width, height])
            }
            DISPID_GET_STATUS => variant::text(&self.status_text()),
            DISPID_SET_LATEX => {
                let argument = variant::argument(pdispparams, 0);
                let Some(text) = variant::as_text(argument) else {
                    return Err(Error::from_hresult(DISP_E_BADPARAMCOUNT));
                };
                variant::text(&self.set_latex(&text))
            }
            DISPID_SAVE => variant::text(&self.save_now()),
            _ => return Err(Error::from_hresult(DISP_E_MEMBERNOTFOUND)),
        };
        if !pvarresult.is_null() {
            unsafe { pvarresult.write(value) };
        }
        Ok(())
    }
}

/// `DISPID_UNKNOWN`, what a name the object does not know maps to.
const DISPID_UNKNOWN: i32 = -1;

/// `DATA_S_SAMEFORMATETC`, the success code for an unchanged canonical format.
const DATA_S_SAMEFORMATETC: HRESULT = HRESULT(0x0004_0130u32 as i32);

/// The member a name stands for, compared the way automation compares names.
fn dispid_of(name: &str) -> i32 {
    if name.eq_ignore_ascii_case("GetLatex") {
        DISPID_GET_LATEX
    } else if name.eq_ignore_ascii_case("SetLatex") {
        DISPID_SET_LATEX
    } else if name.eq_ignore_ascii_case("Save") {
        DISPID_SAVE
    } else if name.eq_ignore_ascii_case("GetSize") {
        DISPID_GET_SIZE
    } else if name.eq_ignore_ascii_case("GetStatus") {
        DISPID_GET_STATUS
    } else {
        DISPID_UNKNOWN
    }
}

/// A wide string the caller frees with `CoTaskMemFree`, which is what `GetUserType` hands out.
fn task_memory_string(text: &str) -> PWSTR {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let buffer =
            windows::Win32::System::Com::CoTaskMemAlloc(wide.len() * std::mem::size_of::<u16>());
        if buffer.is_null() {
            return PWSTR::null();
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), buffer.cast::<u16>(), wide.len());
        PWSTR(buffer.cast::<u16>())
    }
}

/// The equation a new object holds, and the text that spells it.
fn default_equation() -> (Math, String) {
    let latex = DEFAULT_LATEX.to_owned();
    match mt_tex::parse(&latex) {
        Ok(math) => (math, latex),
        // The default text is checked by the tests below; falling back keeps the object usable
        // even if it is ever changed into something the parser rejects.
        Err(_) => (Math::default(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_LATEX, DISPID_GET_LATEX, DISPID_UNKNOWN, Inner, OLECLOSE_SAVEIFDIRTY,
        default_equation, dispid_of, opens_editor, save_on_close,
    };
    use windows::Win32::System::Ole::{
        OLECLOSE_NOSAVE, OLECLOSE_PROMPTSAVE, OLEIVERB_HIDE, OLEIVERB_OPEN, OLEIVERB_PRIMARY,
        OLEIVERB_SHOW,
    };

    #[test]
    fn the_default_equation_is_something_the_parser_accepts() {
        let (math, latex) = default_equation();
        assert_eq!(latex, DEFAULT_LATEX);
        // The canonical spelling drops the spaces the default text carries.
        assert_eq!(mt_tex::to_latex(&math), "y=x^2");
    }

    #[test]
    fn automation_names_are_matched_without_regard_to_case() {
        assert_eq!(dispid_of("getlatex"), DISPID_GET_LATEX);
        assert_eq!(dispid_of("GetLatex"), DISPID_GET_LATEX);
        assert_eq!(dispid_of("nonesuch"), DISPID_UNKNOWN);
    }

    #[test]
    fn closing_saves_only_when_asked_and_dirty() {
        // `OLECLOSE_SAVEIFDIRTY` on a dirty object is the one combination that writes.
        assert!(save_on_close(OLECLOSE_SAVEIFDIRTY, true));
        assert!(!save_on_close(OLECLOSE_SAVEIFDIRTY, false));
        // `NOSAVE` and `PROMPTSAVE` never write the streams on the way out.
        assert!(!save_on_close(OLECLOSE_NOSAVE, true));
        assert!(!save_on_close(OLECLOSE_PROMPTSAVE, true));
    }

    #[test]
    fn every_registered_verb_opens_the_editor() {
        // The standard activation verbs a double-click or an "Open" sends.
        assert!(opens_editor(OLEIVERB_PRIMARY.0));
        assert!(opens_editor(OLEIVERB_SHOW.0));
        assert!(opens_editor(OLEIVERB_OPEN.0));
        // Positive verbs are indexes into the registry list (`Verb\1` is `&Edit`), and Word
        // calls them by number, so they must not be refused with `OLE_E_OLEVERB`.
        assert!(opens_editor(1));
        assert!(opens_editor(7));
        // Hiding is not opening; the verb handling falls through to it.
        assert!(!opens_editor(OLEIVERB_HIDE.0));
    }

    #[test]
    fn restoring_keeps_the_preferences_the_stream_carried() {
        let (math, _) = default_equation();
        let stream = mt_mtef::write(&math);
        let expected_bytes = stream.clone();
        let expected = mt_mtef::read(&stream)
            .expect("the stream parses")
            .header
            .preferences;

        let mut inner = Inner::new();
        assert_eq!(inner.preferences, None);
        inner.restore(None, Some(stream));
        // Regression: the preferences the document wrote the equation with used to be thrown
        // away at load, so a save wrote the defaults back over them.
        assert_eq!(inner.preferences, Some(expected));
        // The retained preferences are what the save path passes to `write_with_preferences`,
        // so they must reproduce the stream the document held.
        assert_eq!(
            mt_mtef::write_with_preferences(&inner.math, inner.preferences.as_ref().unwrap()),
            expected_bytes
        );
    }
}
