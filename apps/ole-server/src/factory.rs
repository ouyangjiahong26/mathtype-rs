//! The class factory COM asks for its object, and the bookkeeping that decides when this server
//! has nothing left to do.
//!
//! A host may create the object, let go of it, and ask for it again; the server therefore only
//! ends its message loop once no object exists, no host has locked the factory, and a short
//! settling time has passed without any activity. That settle time is what keeps a call that
//! arrives just after the last release from landing on a server that has already decided to stop.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::CLASS_E_NOAGGREGATION;
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, CoRegisterClassObject, IClassFactory, IClassFactory_Impl,
    REGCLS_MULTIPLEUSE, REGCLS_SUSPENDED,
};
use windows::core::{BOOL, Error, GUID, Interface, Ref, Result as WinResult, implement};

use crate::object::EquationObject;
use crate::trace;

/// How long a server with nothing to do waits before it lets its message loop end.
pub const IDLE_GRACE: Duration = Duration::from_secs(5);

/// How long it waits after the last object was closed, which is the moment a host is most likely
/// to make one last call.
pub const CLOSE_GRACE: Duration = Duration::from_secs(1);

/// What the server knows about itself while it runs.
///
/// The deadline is milliseconds since the server started, with zero meaning "there is work"; it is
/// a plain counter rather than a lock because it is only ever read and written by COM's calls into
/// this server and by the timer that watches it, and because nothing here may block.
pub struct ServerState {
    started: Instant,
    /// Objects handed to hosts and not yet released.
    live: AtomicU32,
    /// Hosts holding the factory open, which is how COM says "an object is on its way".
    locks: AtomicU32,
    /// Whether anything ever arrived. A host that starts the server and never calls into it leaves
    /// the other counters at zero, which is the case the log has to tell apart from a host that
    /// took an object and let go of it again.
    busy: AtomicBool,
    deadline_ms: AtomicU64,
}

impl ServerState {
    /// A server that has just started and is waiting for its first object.
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            live: AtomicU32::new(0),
            locks: AtomicU32::new(0),
            busy: AtomicBool::new(false),
            deadline_ms: AtomicU64::new(0),
        }
    }

    /// There is something to do, so the loop must keep running.
    pub fn note_busy(&self) {
        self.busy.store(true, Ordering::Relaxed);
        self.deadline_ms.store(0, Ordering::Relaxed);
    }

    /// Nothing is left; the loop may end once `grace` has passed.
    pub fn note_idle(&self, grace: Duration) {
        let candidate = (self.started.elapsed() + grace).as_millis() as u64;
        let mut current = self.deadline_ms.load(Ordering::Relaxed);
        loop {
            if current != 0 && current <= candidate {
                return;
            }
            match self.deadline_ms.compare_exchange_weak(
                current,
                candidate.max(1),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(seen) => current = seen,
            }
        }
    }

    /// One more object exists.
    pub fn object_created(&self) {
        self.live.fetch_add(1, Ordering::Relaxed);
        self.note_busy();
    }

    /// An object was released.
    pub fn object_released(&self) {
        self.live.fetch_sub(1, Ordering::Relaxed);
        self.settle(IDLE_GRACE);
    }

    /// A host closed its object without releasing it.
    pub fn object_closed(&self) {
        self.settle(CLOSE_GRACE);
    }

    /// A host locked or unlocked the factory.
    pub fn lock(&self, flock: bool) {
        if flock {
            self.locks.fetch_add(1, Ordering::Relaxed);
            self.note_busy();
        } else {
            self.locks.fetch_sub(1, Ordering::Relaxed);
            self.settle(IDLE_GRACE);
        }
    }

    /// Whether the message loop may end now.
    pub fn may_quit(&self) -> bool {
        if self.live.load(Ordering::Relaxed) != 0 || self.locks.load(Ordering::Relaxed) != 0 {
            return false;
        }
        let deadline = self.deadline_ms.load(Ordering::Relaxed);
        deadline != 0 && self.started.elapsed().as_millis() as u64 >= deadline
    }

    /// Objects still in the hands of hosts.
    pub fn live(&self) -> u32 {
        self.live.load(Ordering::Relaxed)
    }

    /// Whether anything ever arrived, which tells "a host never called" apart from "an object came
    /// and went" in the log.
    pub fn ever_busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }

    /// Hosts holding the factory open right now.
    pub fn locks(&self) -> u32 {
        self.locks.load(Ordering::Relaxed)
    }

    fn settle(&self, grace: Duration) {
        if self.live.load(Ordering::Relaxed) == 0 && self.locks.load(Ordering::Relaxed) == 0 {
            self.note_idle(grace);
        }
    }
}

impl Default for ServerState {
    fn default() -> Self {
        Self::new()
    }
}

/// Register a factory for the equation class and hand it back, so it stays alive for as long as
/// the server runs.
pub fn register(state: Arc<ServerState>) -> WinResult<(IClassFactory, u32)> {
    let factory: IClassFactory = EquationFactory::new(state).into();
    let cookie = unsafe {
        CoRegisterClassObject(
            &crate::clsid::CLSID_EQUATION,
            &factory,
            CLSCTX_LOCAL_SERVER,
            REGCLS_MULTIPLEUSE | REGCLS_SUSPENDED,
        )
    }?;
    Ok((factory, cookie))
}

/// The factory COM gets when it asks for [`crate::clsid::CLSID_EQUATION`].
#[implement(IClassFactory, Agile = false)]
pub struct EquationFactory {
    state: Arc<ServerState>,
}

impl EquationFactory {
    /// A factory that counts the objects it hands out in `state`.
    pub fn new(state: Arc<ServerState>) -> Self {
        Self { state }
    }
}

impl IClassFactory_Impl for EquationFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Ref<windows::core::IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut core::ffi::c_void,
    ) -> WinResult<()> {
        trace::log(&format!(
            "CreateInstance(riid={}, outer={})",
            if riid.is_null() {
                "null".to_owned()
            } else {
                crate::clsid::id_string(unsafe { &*riid })
            },
            if punkouter.is_null() {
                "null"
            } else {
                "a site"
            }
        ));
        if !punkouter.is_null() {
            // The object does not aggregate anyone else's `IUnknown`, and says so.
            trace::log("CreateInstance refused: the object does not aggregate");
            return Err(Error::from_hresult(CLASS_E_NOAGGREGATION));
        }
        let object = EquationObject::new(self.state.clone());
        let unknown: windows::core::IUnknown = object.into();
        // So an automation client can reach the object by name with `GetActiveObject`.
        crate::object::register_active(&unknown);
        match unsafe { Interface::query(&unknown, riid, ppvobject) }.ok() {
            Ok(()) => {
                trace::log("CreateInstance -> the requested interface");
                Ok(())
            }
            Err(error) => {
                let text = crate::storage::error_text(&error);
                trace::log(&format!(
                    "CreateInstance cannot return the interface: {text}"
                ));
                Err(error)
            }
        }
    }

    fn LockServer(&self, flock: BOOL) -> WinResult<()> {
        trace::log(&format!("LockServer(flock={})", flock.as_bool()));
        self.state.lock(flock.as_bool());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ServerState;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn a_server_with_no_objects_settles_and_then_may_quit() {
        let state = ServerState::new();
        assert!(
            !state.may_quit(),
            "a server that has not settled may not quit"
        );
        state.object_created();
        assert!(!state.may_quit());
        state.object_released();
        // Releasing the last object starts the idle grace; shortening it here keeps the test
        // quick without changing what is being tested.
        state.note_idle(Duration::from_millis(1));
        sleep(Duration::from_millis(20));
        assert!(state.may_quit(), "nothing is left, so the loop may end");
    }

    #[test]
    fn a_locked_factory_keeps_the_server_alive() {
        let state = ServerState::new();
        state.lock(true);
        state.note_idle(Duration::from_millis(1));
        sleep(Duration::from_millis(20));
        assert!(!state.may_quit());
        state.lock(false);
        state.note_idle(Duration::from_millis(1));
        sleep(Duration::from_millis(20));
        assert!(state.may_quit());
    }
}
