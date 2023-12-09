use std::io;
use std::path::Path;
use std::pin::Pin;

use bytes::BufMut;
use io_uring::{opcode, squeue, types};

use crate::fd::NornFd;
use crate::fs::opts;
use crate::operation::{CQEResult, Op, Operation, Singleshot};

/// A reference to an open file on the filesystem.
pub struct File {
    fd: NornFd,
}

impl std::fmt::Debug for File {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("File").finish()
    }
}

impl File {
    /// Open a file with the specified options at the provided
    /// path.
    pub(crate) async fn open_with_options<P: AsRef<Path>>(
        path: P,
        opts: opts::OpenOptions,
    ) -> io::Result<Self> {
        let access_mode = opts.get_access_mode()?;
        let creation_mode = opts.get_creation_mode()?;
        let open = Open::new(path.as_ref(), access_mode, creation_mode)?;
        let handle = crate::Handle::current();
        let fd = Op::new(open, handle).await??;
        Ok(Self { fd })
    }

    /// Open a file in read-only mode at the provided path.
    pub async fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let mut opts = opts::OpenOptions::new();
        opts.read(true).open(path).await
    }

    /// Returns a new [`OpenOptions`] object which can be used to open a file.
    pub fn with_options() -> opts::OpenOptions {
        opts::OpenOptions::new()
    }
}

struct Open {
    path: std::ffi::CString,
    access_mode: i32,
    creation_mode: i32,
}

impl Open {
    fn new(path: &Path, access_mode: i32, creation_mode: i32) -> io::Result<Self> {
        let path = path
            .to_str()
            .ok_or_else(|| io::Error::from_raw_os_error(libc::EINVAL))?;
        let path = std::ffi::CString::new(path)?;
        Ok(Self {
            path,
            access_mode,
            creation_mode,
        })
    }
}

impl Operation for Open {
    fn configure(self: Pin<&mut Self>) -> io_uring::squeue::Entry {
        let this = self.get_mut();
        let ptr = this.path.as_ptr();
        opcode::OpenAt::new(types::Fd(libc::AT_FDCWD), ptr)
            .flags(this.access_mode | this.creation_mode | libc::O_CLOEXEC)
            .build()
    }

    fn cleanup(&mut self, result: CQEResult) {
        if let Ok(res) = result.result {
            drop(NornFd::from_fd(res as _));
        }
    }
}

impl Singleshot for Open {
    type Output = io::Result<NornFd>;

    fn complete(self, result: CQEResult) -> Self::Output {
        let res = result.result?;
        Ok(NornFd::from_fd(res as _))
    }
}

struct ReadAt<B> {
    fd: NornFd,
    buf: B,
    offset: u64,
}

impl<B> Operation for ReadAt<B>
where
    B: BufMut,
{
    fn configure(mut self: Pin<&mut Self>) -> io_uring::squeue::Entry {
        let this = unsafe { self.get_unchecked_mut() };
        let ptr = this.buf.

        match this.fd.kind() {
            crate::fd::FdKind::Fd(fd) => {
                opcode::Read::new(*fd);
            }
            crate::fd::FdKind::Fixed(_) => todo!(),
        }

        todo!()
    }

    fn cleanup(&mut self, result: CQEResult) {
        todo!()
    }
}
