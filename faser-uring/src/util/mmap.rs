use std::{io, ptr};

/// An anonymous region of memory mapped using `mmap(2)`, not backed by a file
/// but that is guaranteed to be page-aligned and zero-filled.
pub(crate) struct AnonymousMmap {
    addr: ptr::NonNull<libc::c_void>,
    len: usize,
}

impl AnonymousMmap {
    /// Creates a new anonymous mapping of `len` bytes.
    pub(crate) fn new(len: usize) -> io::Result<Self> {
        let addr = unsafe {
            match libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_ANONYMOUS | libc::MAP_PRIVATE | libc::MAP_POPULATE,
                0,
                0,
            ) {
                libc::MAP_FAILED => return Err(io::Error::last_os_error()),
                addr => ptr::NonNull::new_unchecked(addr),
            }
        };
        match unsafe { libc::madvise(addr.as_ptr(), len, libc::MADV_DONTFORK) } {
            0 => {
                let mmap = Self { addr, len };
                Ok(mmap)
            }
            _ => Err(io::Error::last_os_error()),
        }
    }

    /// Get a pointer to the memory.
    #[inline]
    pub(crate) fn as_ptr(&self) -> *const libc::c_void {
        self.addr.as_ptr()
    }

    /// Get a mut pointer to the memory.
    #[inline]
    pub(crate) fn as_ptr_mut(&self) -> *mut libc::c_void {
        self.addr.as_ptr()
    }
}

impl Drop for AnonymousMmap {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.addr.as_ptr(), self.len);
        }
    }
}
