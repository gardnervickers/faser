use std::cell::RefCell;

use crate::Handle;

thread_local! {
    static CURRENT: Context = Context::new();
}

pub(crate) struct Context {
    handle: RefCell<Option<Handle>>,
}

impl Context {
    fn new() -> Self {
        Self {
            handle: Default::default(),
        }
    }

    pub(crate) fn enter(handle: Handle) -> ContextGuard {
        CURRENT.with(|current| {
            let mut old = current.handle.borrow_mut();
            assert!(old.is_none(), "executor already set");
            *old = Some(handle);
        });
        ContextGuard {}
    }

    /// Returns a reference to the current executor.
    pub(crate) fn handle() -> Option<Handle> {
        CURRENT.with(|c| c.handle.borrow().clone())
    }
}

#[derive(Debug)]
pub struct ContextGuard;

impl Drop for ContextGuard {
    fn drop(&mut self) {
        CURRENT.with(|current| {
            let mut executor = current.handle.borrow_mut();
            assert!(executor.is_some(), "executor not set");
            *executor = None;
        });
    }
}
