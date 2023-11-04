//! # File Descriptors
//!
//! We need a way to make sure that a file descriptor does not get
//! closed while we are using it. This can be when the app has a
//! reference to the file descriptor, but it can also be when
//! the kernel is using the file descriptor.
//!
//! Essentially we need a reference counted file descriptor.
//!
//! Additionally, io-uring supports two types of file descriptors,
//! regular file descriptors and fixed file descriptors.
use std::mem;
use std::os::fd::{AsRawFd, RawFd};
use std::rc::Rc;

use io_uring::types;

use crate::Handle;

/// [`FaserFd`] is a reference counted file descriptor.
#[derive(Clone, Debug)]
pub(crate) struct FaserFd {
    inner: Rc<Inner>,
}

#[derive(Debug)]
struct Inner {
    kind: FdKind,
}

#[derive(Debug, Clone)]
pub(crate) enum FdKind {
    Fd(types::Fd),
    Fixed(types::Fixed),
}

impl FaserFd {
    /// Create a new [`FaserFd`] from a regular file descriptor.
    pub(crate) fn from_fd(fd: RawFd) -> Self {
        let raw = fd.as_raw_fd();
        mem::forget(fd);
        Self::new(FdKind::Fd(types::Fd(raw)))
    }

    /// Create a new [`FaserFd`] from a fixed file descriptor.
    pub(crate) fn from_fixed(fixed: types::Fixed) -> Self {
        Self::new(FdKind::Fixed(fixed))
    }

    fn new(kind: FdKind) -> Self {
        let inner = Inner { kind };
        let inner = Rc::new(inner);
        Self { inner }
    }

    pub(crate) fn kind(&self) -> &'_ FdKind {
        &self.inner.kind
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let handle = Handle::current();
        if let Err(err) = handle.close_fd(&self.kind) {
            log::error!("failed to close fd: {}", err);
        }
    }
}
