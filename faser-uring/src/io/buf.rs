//! Buf types for interacting with the ring.
//!
//! These are lifted from tokio-uring.
//!
//! TODO: See if it would be possible to expose
//! these traits independently for interop? They
//! seem pretty fundemental for io_uring usage.

use bytes::{Bytes, BytesMut};

pub unsafe trait StableBuf: Unpin + 'static {
    fn stable_ptr(&self) -> *const u8;

    fn bytes_init(&self) -> usize;

    fn bytes_total(&self) -> usize;
}

pub unsafe trait StableBufMut: StableBuf {
    fn stable_mut_ptr(&mut self) -> *mut u8;

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
