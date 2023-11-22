//! Buf types for interacting with the ring.
//!
//! These are lifted from tokio-uring.
//!
//! TODO: See if it would be possible to expose
//! these traits independently for interop? They
//! seem pretty fundemental for io_uring usage.

use bytes::{Bytes, BytesMut};

/// An immutable buffer referencing a stable memory location.
///
/// # Safety
/// Implementors must ensure that the memory location
/// returned by stable_ptr is valid even if the StableBuf
/// is moved.
pub unsafe trait StableBuf: Unpin + 'static {
    /// Return a pointer to the stable memory location.
    fn stable_ptr(&self) -> *const u8;

    fn bytes_init(&self) -> usize;

    fn bytes_total(&self) -> usize;
}

/// A mutable buffer referencing a stable memory location.
///
/// # Safety
/// Implementors must ensure that the memory location
/// returned by stable_mut_ptr is valid even if the StableBufMut
/// is moved.
pub unsafe trait StableBufMut: StableBuf {
    /// Return a mutable pointer to the stable memory location.
    fn stable_mut_ptr(&mut self) -> *mut u8;

    /// Set the number of initialized bytes in the buffer.
    ///
    /// # Safety
    /// The caller must ensure that all bytes `0..pos`
    /// have been initialized.
    unsafe fn set_init(&mut self, pos: usize);
}

unsafe impl StableBuf for Vec<u8> {
    fn stable_ptr(&self) -> *const u8 {
        self.as_ptr()
    }

    fn bytes_init(&self) -> usize {
        self.len()
    }

    fn bytes_total(&self) -> usize {
        self.capacity()
    }
}

unsafe impl StableBufMut for Vec<u8> {
    fn stable_mut_ptr(&mut self) -> *mut u8 {
        self.as_mut_ptr()
    }

    unsafe fn set_init(&mut self, pos: usize) {
        self.set_len(pos);
    }
}

unsafe impl StableBuf for &'static [u8] {
    fn stable_ptr(&self) -> *const u8 {
        self.as_ptr()
    }

    fn bytes_init(&self) -> usize {
        self.len()
    }

    fn bytes_total(&self) -> usize {
        self.len()
    }
}

unsafe impl StableBuf for &'static str {
    fn stable_ptr(&self) -> *const u8 {
        self.as_ptr()
    }

    fn bytes_init(&self) -> usize {
        self.len()
    }

    fn bytes_total(&self) -> usize {
        self.len()
    }
}

unsafe impl StableBuf for Bytes {
    fn stable_ptr(&self) -> *const u8 {
        self.as_ptr()
    }

    fn bytes_init(&self) -> usize {
        self.len()
    }

    fn bytes_total(&self) -> usize {
        self.len()
    }
}

unsafe impl StableBuf for BytesMut {
    fn stable_ptr(&self) -> *const u8 {
        self.as_ptr()
    }

    fn bytes_init(&self) -> usize {
        self.len()
    }

    fn bytes_total(&self) -> usize {
        self.capacity()
    }
}

unsafe impl StableBufMut for BytesMut {
    fn stable_mut_ptr(&mut self) -> *mut u8 {
        self.as_mut_ptr()
    }

    unsafe fn set_init(&mut self, pos: usize) {
        self.set_len(pos);
    }
}
