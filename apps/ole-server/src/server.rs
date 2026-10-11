//! Running as the local server a document host starts with `-Embedding`.
//!
//! The thread that COM starts the object on has to be a single-threaded apartment, so the whole
//! server is one thread: it initializes OLE, registers the class factory, and then sits in a
//! message loop that serves the host's calls. The loop also carries a timer, which is how a
//! finished editor session gets read back and written into the host's storage (see
//! [`crate::object::tick_all`]), and it is what decides that there is nothing left to do.

use std::sync::Arc;

use windows::Win32::System::Com::{CoResumeClassObjects, CoRevokeClassObject};
use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, KillTimer, MSG, PostQuitMessage, SetTimer, TranslateMessage, WM_TIMER,
};
use windows::core::BOOL;

use crate::factory::{self, ServerState};
use crate::object;
use crate::storage::error_text;
use crate::trace;

/// How often the loop looks at the objects, in milliseconds.
const TICK_MS: u32 = 200;

/// How long a server that is started but never asked for an object keeps waiting.
const STARTUP_GRACE: std::time::Duration = std::time::Duration::from_secs(30);

/// Serve the equation class until no host needs it any more.
pub fn run() -> Result<(), String> {
    unsafe { OleInitialize(None) }.map_err(|error| format!("OleInitialize failed: {}", error_text(&error)))?;
    trace::log("serving the equation class");

    let state = Arc::new(ServerState::new());
    let (factory, cookie) = factory::register(state.clone())
        .map_err(|error| format!("CoRegisterClassObject failed: {}", error_text(&error)))?;
    unsafe { CoResumeClassObjects() }
        .map_err(|error| format!("CoResumeClassObjects failed: {}", error_text(&error)))?;
    // A host that starts this server without ever asking for an object must not leave it running.
    state.note_idle(STARTUP_GRACE);

    let timer = unsafe { SetTimer(None, 0, TICK_MS, None) };
    if timer == 0 {
        trace::log("no timer, so a finished editor session will not be read back");
    }

    let mut message = MSG::default();
    loop {
        let status: BOOL = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if status.0 <= 0 {
            // Zero is `WM_QUIT`; minus one is the loop giving up because of an error.
            if status.0 < 0 {
                trace::log("GetMessage failed; ending the message loop");
            }
            break;
        }
        if message.hwnd.0.is_null() {
            // A message for this thread rather than for one of its windows: the timer, or the
            // host's request to stop through `WM_QUIT`, which `GetMessage` turns into a zero.
            if message.message == WM_TIMER {
                on_timer(&state);
            }
            continue;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    if timer != 0 {
        let _ = unsafe { KillTimer(None, timer) };
    }
    object::revoke_active();
    let _ = unsafe { CoRevokeClassObject(cookie) };
    unsafe { OleUninitialize() };
    trace::log(&format!("stopped serving ({} objects, factory {factory:?} released)", state.live()));
    Ok(())
}

/// One turn of the timer: read back finished editor sessions, then see whether anything is left.
fn on_timer(state: &ServerState) {
    object::tick_all();
    if state.may_quit() {
        trace::log("no objects and nothing pending; ending the message loop");
        unsafe { PostQuitMessage(0) };
    }
}

/// The grace period used when nothing ever arrives, which is what the tests below check.
#[cfg(test)]
mod tests {
    use super::{STARTUP_GRACE, TICK_MS};
    use crate::factory::IDLE_GRACE;

    #[test]
    fn the_timings_are_what_the_message_loop_and_the_contract_expect() {
        assert!(TICK_MS > 0 && TICK_MS <= 1000);
        assert!(STARTUP_GRACE > IDLE_GRACE);
    }
}
