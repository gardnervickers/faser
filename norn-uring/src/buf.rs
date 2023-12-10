//! Traits for I/O buffers.
use bytes::{Bytes, BytesMut};

/// [`StableBuf`] is a trait for types which expose a
/// stable pointer into initialized memory.
///
/// ### Safety
/// Implementors of this trait must ensure that the pointer returned by
/// stable_ptr is valid and points to initialized memory of at least
/// bytes_init bytes.
///
/// Furthermore, the pointer must remain valid for the lifetime of the
/// request it is used in, and must not be moved.
pub unsafe trait StableBuf: Unpin + 'static {
    /// Returns a pointer to the stable memory location.
    fn stable_ptr(&self) -> *const u8;

    /// Returns the number of initialized bytes.
    fn bytes_init(&self) -> usize;
}

/// [`StableBufMut`] is a trait for types which expose a
/// stable pointer into memory.
///
/// ### Safety
/// Implementors of this trait must ensure that the pointer returned by
/// stable_ptr_mut is valid.
pub unsafe trait StableBufMut: Unpin + 'static {
    /// Returns a mutable pointer to the stable memory location.
    fn stable_ptr_mut(&mut self) -> *mut u8;

    /// Returns the capacity of the buffer.
    fn bytes_remaining(&self) -> usize;

    /// Set the number of initialized bytes.
    ///
    /// ### Safety
    /// Callers should ensure that all bytes from 0..init_len are initialized.
    unsafe fn set_init(&mut self, init_len: usize);
}

unsafe impl StableBuf for Vec<u8> {
    fn stable_ptr(&self) -> *const u8 {
        self.as_ptr()
    }

    fn bytes_init(&self) -> usize {
        self.len()
    }
}

unsafe impl StableBufMut for Vec<u8> {
    fn stable_ptr_mut(&mut self) -> *mut u8 {
        self.as_mut_ptr()
    }

    fn bytes_remaining(&self) -> usize {
        self.capacity()
    }

    unsafe fn set_init(&mut self, init_len: usize) {
        self.set_len(init_len);
    }
}

unsafe impl StableBuf for Box<[u8]> {
    fn stable_ptr(&self) -> *const u8 {
        self.as_ptr()
    }

    fn bytes_init(&self) -> usize {
        self.len()
    }
}

unsafe impl StableBufMut for Box<[u8]> {
    fn stable_ptr_mut(&mut self) -> *mut u8 {
        self.as_mut_ptr()
    }

    fn bytes_remaining(&self) -> usize {
        self.len()
    }

    unsafe fn set_init(&mut self, _: usize) {}
}

unsafe impl StableBuf for &'static [u8] {
    fn stable_ptr(&self) -> *const u8 {
        self.as_ptr()
    }

    fn bytes_init(&self) -> usize {
        self.len()
    }
}

unsafe impl StableBuf for &'static str {
    fn stable_ptr(&self) -> *const u8 {
        self.as_ptr()
    }

    fn bytes_init(&self) -> usize {
        str::len(self)
    }
}

unsafe impl StableBuf for Bytes {
    fn stable_ptr(&self) -> *const u8 {
        self.as_ptr()
    }

    fn bytes_init(&self) -> usize {
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
}

unsafe impl StableBufMut for BytesMut {
    fn stable_ptr_mut(&mut self) -> *mut u8 {
        self.as_mut_ptr()
    }

    fn bytes_remaining(&self) -> usize {
        self.capacity()
    }

    unsafe fn set_init(&mut self, init_len: usize) {
        if self.len() < init_len {
            self.set_len(init_len)
        }
    }
}
