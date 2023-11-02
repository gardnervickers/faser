use std::sync::{Arc, Condvar, Mutex};

use crate::park::{Park, ParkMode, Unpark};

/// [`Park`] implementation that will park the
/// calling thread on a [`Condvar`] and wake it
/// when [`Unpark`] is called.
#[derive(Default)]
pub struct ThreadPark {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for ThreadPark {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadPark").finish()
    }
}

#[derive(Clone)]
pub struct ThreadUnpark {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for ThreadUnpark {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadUnpark").finish()
    }
}

#[derive(Default)]
struct Inner {
    mutex: Mutex<()>,
    condvar: Condvar,
}

impl Unpark for ThreadUnpark {
    fn unpark(&self) {
        self.inner.unpark();
    }
}

impl Park for ThreadPark {
    type Unparker = ThreadUnpark;

    type Guard = ();

    fn park(&mut self, mode: ParkMode) -> Result<(), std::io::Error> {
        self.inner.park(mode);
        Ok(())
    }

    fn enter(&self) -> Self::Guard {}

    fn unparker(&self) -> Self::Unparker {
        ThreadUnpark {
            inner: Arc::clone(&self.inner),
        }
    }

    fn needs_park(&self) -> bool {
        false
    }

    fn shutdown(&mut self) {}
}

impl Inner {
    fn unpark(&self) {
        self.condvar.notify_all();
    }

    fn park(&self, mode: ParkMode) {
        match mode {
            ParkMode::NoPark => (),
            ParkMode::NextCompletion => {
                let _guard = self.mutex.lock().unwrap();
                let _unused = self.condvar.wait(_guard).unwrap();
            }
            ParkMode::Timeout(timeout) => {
                let _guard = self.mutex.lock().unwrap();
                let _unused = self.condvar.wait_timeout(_guard, timeout).unwrap();
            }
        };
    }
}
