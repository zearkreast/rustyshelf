//! Work off the frame, on threads of its own.

use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
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

/// A thread that works through what's asked of it in turn, for as long as its answers are wanted. With `newest`,
/// asks that came in while it worked give way to the last of them. `work` answering None sends nothing.
pub fn worker<A: Send + 'static, R: Send + 'static>(newest: bool, mut work: impl FnMut(A) -> Option<R> + Send + 'static) -> (Sender<A>, Mutex<Receiver<R>>) {
    let (ask, asked) = channel::<A>();
    let (tx, done) = channel();
    std::thread::spawn(move || {
        while let Ok(mut a) = asked.recv() {
            if newest {
                while let Ok(newer) = asked.try_recv() {
                    a = newer;
                }
            }
            if let Some(r) = work(a) {
                if tx.send(r).is_err() {
                    return;
                }
            }
        }
    });
    (ask, Mutex::new(done))
}
