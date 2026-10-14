//! What the server says when it dies on its own.
//!
//! A local server has no console and no supervisor, so a fault inside a COM call ends the process
//! without a word: the host waits, the log stops mid-sentence, and nothing says whether the object
//! panicked, ran off a pointer or was killed. This module turns both ways a Rust process can die
//! into log lines, because those lines are the only evidence there is afterwards.
//!
//! A panic is logged with its message and location. An unhandled Windows exception is logged with
//! its code, the address that faulted, what the fault was reaching for, and the return addresses
//! that were on the stack, which together say which call was in flight when it happened.

use std::ffi::c_void;

use windows::Win32::System::Diagnostics::Debug::{
    EXCEPTION_CONTINUE_SEARCH, EXCEPTION_POINTERS, RtlCaptureStackBackTrace,
    SetUnhandledExceptionFilter,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::trace;

/// How many return addresses are worth logging.
const FRAMES: usize = 24;

/// Report a panic or an unhandled exception through the log, then let the process die as it would
/// have. Called once, before the server does anything else.
pub fn install() {
    std::panic::set_hook(Box::new(|info| {
        trace::log(&format!("panic: {info}"));
    }));
    unsafe { SetUnhandledExceptionFilter(Some(report)) };
}

/// The last thing this process writes: the exception, the addresses that were on the stack.
unsafe extern "system" fn report(pointers: *const EXCEPTION_POINTERS) -> i32 {
    if pointers.is_null() {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    let record = unsafe { (*pointers).ExceptionRecord };
    if !record.is_null() {
        let code = unsafe { (*record).ExceptionCode.0 } as u32;
        let address = unsafe { (*record).ExceptionAddress } as usize;
        trace::log(&format!("fatal exception {code:#010X} at {address:#x}"));
        // For an access violation the first parameter says whether it was a read, a write or an
        // instruction fetch, and the second is the address that was reached for.
        let parameters = unsafe { (*record).NumberParameters } as usize;
        for index in 0..parameters.min(15) {
            let value = unsafe { (*record).ExceptionInformation[index] };
            trace::log(&format!("  exception parameter {index}: {value:#x}"));
        }
    }
    // The base of this program's own image is logged with the frames so a reader can tell a fault
    // in the server apart from one in COM's own stubs, which are loaded elsewhere.
    let base = unsafe { GetModuleHandleW(None) }
        .map(|module| module.0 as usize)
        .unwrap_or(0);
    trace::log(&format!("this program is loaded at {base:#x}"));
    let mut frames = [core::ptr::null_mut::<c_void>(); FRAMES];
    let count = unsafe { RtlCaptureStackBackTrace(0, &mut frames, None) } as usize;
    for (index, frame) in frames.iter().take(count.min(FRAMES)).enumerate() {
        let frame = *frame as usize;
        if base != 0 && frame >= base {
            trace::log(&format!(
                "  frame {index}: {frame:#x}, this program +{:#x}",
                frame - base
            ));
        } else {
            trace::log(&format!("  frame {index}: {frame:#x}"));
        }
    }
    EXCEPTION_CONTINUE_SEARCH
}

#[cfg(test)]
mod tests {
    use super::install;

    /// Run by hand to check that the reporter writes: a fault is the one thing the tests cannot
    /// observe on their own, because the process it happens in does not survive it.
    ///
    /// ```text
    /// cargo test -p mathtype-ole-server -- --ignored --exact fatal::tests::the_reporter_logs_a_fault
    /// mathtype-ole-server.exe --log 20
    /// ```
    #[test]
    #[ignore = "raises an access violation on purpose; run it by hand and read the log"]
    fn the_reporter_logs_a_fault() {
        install();
        unsafe { std::ptr::write_volatile(std::ptr::null_mut::<u8>(), 1) };
    }
}
