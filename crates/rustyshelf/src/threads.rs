//! Work off the frame, on threads of its own.

use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Mutex;

/// Work on a thread of its own, and the one answer it comes to.
pub struct Later<T>(Mutex<Receiver<T>>);

impl<T: Send + 'static> Later<T> {
    pub fn spawn(work: impl FnOnce() -> T + Send + 'static) -> Later<T> {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        Later(Mutex::new(rx))
    }
}

impl<T> Later<T> {
    /// The answer, once it's in.
    pub fn poll(&self) -> Option<T> {
        self.0.lock().ok()?.try_recv().ok()
    }

    /// The answer once it's in; Disconnected when the thread died without one.
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.0.lock().unwrap().try_recv()
    }
}
