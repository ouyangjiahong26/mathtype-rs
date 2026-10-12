//! Telling the host's advise sinks that the object was saved, without making the host's answer
//! this apartment's problem.
//!
//! A container's advise sinks belong to the container's apartment, and the container is likely to
//! be busy with this server exactly when the news is ready: it may be inside its own call into the
//! object, or saving the document the object sits in. A notification sent from the apartment that
//! has to answer that call would wait for the container while the container waits for the
//! apartment, and both would stop for good. The interfaces are therefore marshalled into a thread
//! of their own: the waiting happens there, where a container that is busy costs nothing.
//!
//! Only what the sinks are for is said from here. The host is not asked to save its document,
//! because a container told to save while it is already saving does not come out of it; what tells
//! the host that the document has changes is `IPersistStorage::IsDirty`, which stays true until
//! the host saves the object.

use std::sync::LazyLock;
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};

use windows::Win32::System::Com::Marshal::CoMarshalInterThreadInterfaceInStream;
use windows::Win32::System::Com::StructuredStorage::CoGetInterfaceAndReleaseStream;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, IAdviseSink, IStream};
use windows::core::{IUnknown, Interface};

use crate::storage::error_text;
use crate::trace;

/// How many calls may be waiting for the host before new ones are dropped.
const QUEUE: usize = 32;

/// One call to make on the host, with the marshalled interface it is for.
enum Notice {
    OnSave(Stream),
    OnViewChange(Stream, u32),
}

/// A marshalled interface on its way to the worker thread.
struct Stream(IStream);

// SAFETY: this is the value COM itself hands out to move an interface between apartments, and the
// worker unmarshals it into an apartment of its own before anything is called on it.
unsafe impl Send for Stream {}

/// The calls waiting for the host, drained by a thread of their own.
static TO_HOST: LazyLock<SyncSender<Notice>> = LazyLock::new(|| {
    let (sender, receiver) = sync_channel::<Notice>(QUEUE);
    std::thread::spawn(move || {
        // The worker needs an apartment to unmarshal into. It never pumps messages, which is what
        // makes it safe: every call it makes goes out, and none of them is answered by it.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        for notice in receiver {
            deliver(notice);
        }
    });
    sender
});

/// Tell the host's advise sink that the object was saved.
pub fn on_save(sink: &IAdviseSink) {
    send(marshal("advise sink", sink).map(Notice::OnSave));
}

/// Tell the host's advise sink that the object's picture changed.
pub fn on_view_change(sink: &IAdviseSink, aspects: u32) {
    send(marshal("advise sink", sink).map(|stream| Notice::OnViewChange(stream, aspects)));
}

/// Hand a notice to the worker thread, dropping it when the host is too far behind to care.
fn send(notice: Option<Notice>) {
    let Some(notice) = notice else {
        return;
    };
    match TO_HOST.try_send(notice) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            trace::log("the host is not answering: a call to it was dropped");
        }
        // The worker is gone, which happens when the process is on its way out anyway.
        Err(TrySendError::Disconnected(_)) => {}
    }
}

/// Marshal an interface so another apartment can call it.
fn marshal<T: Interface>(name: &str, interface: &T) -> Option<Stream> {
    // `CoMarshalInterThreadInterfaceInStream` takes the interface as `IUnknown`, and asking for it
    // is answered by the proxy itself rather than by the host.
    let unknown: IUnknown = match interface.cast() {
        Ok(unknown) => unknown,
        Err(error) => {
            trace::log(&format!(
                "the host's {name} is not an `IUnknown`: {}",
                error_text(&error)
            ));
            return None;
        }
    };
    match unsafe { CoMarshalInterThreadInterfaceInStream(&T::IID, &unknown) } {
        Ok(stream) => Some(Stream(stream)),
        Err(error) => {
            trace::log(&format!(
                "the host's {name} cannot be marshalled: {}",
                error_text(&error)
            ));
            None
        }
    }
}

/// Make one call on the host, on the worker thread.
fn deliver(notice: Notice) {
    match notice {
        Notice::OnSave(stream) => {
            if let Some(sink) = unmarshal::<IAdviseSink>("advise sink", stream) {
                unsafe { sink.OnSave() };
            }
        }
        Notice::OnViewChange(stream, aspects) => {
            if let Some(sink) = unmarshal::<IAdviseSink>("advise sink", stream) {
                unsafe { sink.OnViewChange(aspects, -1) };
            }
        }
    }
}

/// Take a marshalled interface back, in the worker's own apartment.
fn unmarshal<T: Interface>(name: &str, stream: Stream) -> Option<T> {
    match unsafe { CoGetInterfaceAndReleaseStream::<_, T>(&stream.0) } {
        Ok(interface) => Some(interface),
        Err(error) => {
            trace::log(&format!(
                "the host's {name} cannot be taken back: {}",
                error_text(&error)
            ));
            None
        }
    }
}
