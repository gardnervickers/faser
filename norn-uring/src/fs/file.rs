use std::io;
use std::path::Path;
use std::pin::Pin;

use io_uring::types::FsyncFlags;
use io_uring::{opcode, types};

use crate::buf::{StableBuf, StableBufMut};
use crate::fd::{FdKind, NornFd};
use crate::fs::opts;
use crate::operation::{CQEResult, Operation, Singleshot};

/// A reference to an open file on the filesystem.
pub struct File {
    fd: NornFd,
    handle: crate::Handle,
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
        let fd = handle.submit(open).await?;
        Ok(Self { fd, handle })
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

    /// Read bytes from the file into the specified buffer.
    ///
    /// The read will start at the provided offset.
    pub async fn read_at<B>(&self, buf: B, offset: u64) -> (io::Result<usize>, B)
    where
        B: StableBufMut + 'static,
    {
        let read = ReadAt::new(self.fd.clone(), buf, offset);
        self.handle.submit(read).await
    }

    /// Write the specified buffer to the file.
    ///
    /// The write will start at the provided offset.
    pub async fn write_at<B>(&self, buf: B, offset: u64) -> (io::Result<usize>, B)
    where
        B: StableBuf + 'static,
    {
        let write = WriteAt::new(self.fd.clone(), buf, offset);
        self.handle.submit(write).await
    }

    /// Sync the file and metadata to disk.
    pub async fn sync(&self) -> io::Result<()> {
        let flags = FsyncFlags::empty();
        let sync = Sync::new(self.fd.clone(), flags);
        self.handle.submit(sync).await
    }

    /// Sync only the data in the file to disk.
    pub async fn datasync(&self) -> io::Result<()> {
        let flags = FsyncFlags::DATASYNC;
        let sync = Sync::new(self.fd.clone(), flags);
        self.handle.submit(sync).await
    }

    /// Sync a range of the file.
    pub async fn sync_range(&self, offset: u64, len: u32, flags: u32) -> io::Result<()> {
        let sync = SyncRange::new(self.fd.clone(), offset, len, flags);
        self.handle.submit(sync).await
    }

    /// Call `fallocate` on the file.
    pub async fn fallocate(&self, offset: u64, len: u64, mode: i32) -> io::Result<()> {
        let fallocate = Fallocate::new(self.fd.clone(), offset, len, mode);
        self.handle.submit(fallocate).await
    }
    /// Allocate additional space in the file without changing the file length metadata.
    ///
    /// This is akin to fallocate with `FALLOC_FL_ZERO_RANGE` and `FALLOC_FL_KEEP_SIZE` set.
    pub async fn allocate(&self, offset: u64, len: u64) -> io::Result<()> {
        self.fallocate(
            offset,
            len,
            libc::FALLOC_FL_ZERO_RANGE | libc::FALLOC_FL_KEEP_SIZE,
        )
        .await
    }
    /// Punches a hole in the file at the specified offset range.
    ///
    /// This can be used to discard portions of a file to save space. The file size will not be
    /// changed.
    pub async fn discard(&self, offset: u64, len: u64) -> io::Result<()> {
        self.fallocate(
            offset,
            len,
            libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE,
        )
        .await
    }

    /// Close the file.
    pub async fn close(self) -> io::Result<()> {
        self.fd.close().await
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

#[derive(Debug)]
struct ReadAt<B> {
    fd: NornFd,
    buf: B,
    offset: u64,
}

impl<B> ReadAt<B> {
    fn new(fd: NornFd, buf: B, offset: u64) -> Self {
        Self { fd, buf, offset }
    }
}

impl<B> Operation for ReadAt<B>
where
    B: StableBufMut,
{
    fn configure(mut self: Pin<&mut Self>) -> io_uring::squeue::Entry {
        let buf = self.buf.stable_ptr_mut();
        let len = self.buf.bytes_remaining();
        match self.fd.kind() {
            FdKind::Fd(fd) => opcode::Read::new(*fd, buf, len as _),
            FdKind::Fixed(fd) => opcode::Read::new(*fd, buf, len as _),
        }
        .offset(self.offset)
        .build()
    }

    fn cleanup(&mut self, _: CQEResult) {}
}

impl<B> Singleshot for ReadAt<B>
where
    B: StableBufMut,
{
    type Output = (io::Result<usize>, B);

    fn complete(mut self, result: CQEResult) -> Self::Output {
        match result.result {
            Ok(n) => {
                let n = n as usize;
                unsafe {
                    self.buf.set_init(n);
                }
                (Ok(n), self.buf)
            }
            Err(err) => {
                let buf = self.buf;
                (Err(err), buf)
            }
        }
    }
}

struct WriteAt<B> {
    fd: NornFd,
    buf: B,
    offset: u64,
}

impl<B> WriteAt<B> {
    fn new(fd: NornFd, buf: B, offset: u64) -> Self {
        Self { fd, buf, offset }
    }
}

impl<B> Operation for WriteAt<B>
where
    B: StableBuf,
{
    fn configure(self: Pin<&mut Self>) -> io_uring::squeue::Entry {
        let buf = self.buf.stable_ptr();
        let len = self.buf.bytes_init();
        match self.fd.kind() {
            FdKind::Fd(fd) => opcode::Write::new(*fd, buf, len as _),
            FdKind::Fixed(fd) => opcode::Write::new(*fd, buf, len as _),
        }
        .offset(self.offset)
        .build()
    }

    fn cleanup(&mut self, _: CQEResult) {}
}

impl<B> Singleshot for WriteAt<B>
where
    B: StableBuf,
{
    type Output = (io::Result<usize>, B);

    fn complete(self, result: CQEResult) -> Self::Output {
        match result.result {
            Ok(n) => (Ok(n as usize), self.buf),
            Err(err) => (Err(err), self.buf),
        }
    }
}

struct Sync {
    fd: NornFd,
    flags: FsyncFlags,
}

impl Sync {
    fn new(fd: NornFd, flags: FsyncFlags) -> Self {
        Self { fd, flags }
    }
}

impl Operation for Sync {
    fn configure(self: Pin<&mut Self>) -> io_uring::squeue::Entry {
        match self.fd.kind() {
            FdKind::Fd(fd) => opcode::Fsync::new(*fd),
            FdKind::Fixed(fd) => opcode::Fsync::new(*fd),
        }
        .flags(self.flags)
        .build()
    }

    fn cleanup(&mut self, _: CQEResult) {}
}

impl Singleshot for Sync {
    type Output = io::Result<()>;

    fn complete(self, result: CQEResult) -> Self::Output {
        result.result.map(|_| ())
    }
}
struct SyncRange {
    fd: NornFd,
    offset: u64,
    len: u32,
    flags: u32,
}

impl SyncRange {
    fn new(fd: NornFd, offset: u64, len: u32, flags: u32) -> Self {
        Self {
            fd,
            offset,
            len,
            flags,
        }
    }
}

impl Operation for SyncRange {
    fn configure(self: Pin<&mut Self>) -> io_uring::squeue::Entry {
        match self.fd.kind() {
            FdKind::Fd(fd) => opcode::SyncFileRange::new(*fd, self.len),
            FdKind::Fixed(fd) => opcode::SyncFileRange::new(*fd, self.len),
        }
        .offset(self.offset)
        .flags(self.flags)
        .build()
    }

    fn cleanup(&mut self, _: CQEResult) {}
}

impl Singleshot for SyncRange {
    type Output = io::Result<()>;

    fn complete(self, result: CQEResult) -> Self::Output {
        result.result.map(|_| ())
    }
}

struct Fallocate {
    fd: NornFd,
    offset: u64,
    len: u64,
    mode: i32,
}

impl Fallocate {
    fn new(fd: NornFd, offset: u64, len: u64, mode: i32) -> Self {
        Self {
            fd,
            offset,
            len,
            mode,
        }
    }
}

impl Operation for Fallocate {
    fn configure(self: Pin<&mut Self>) -> io_uring::squeue::Entry {
        match self.fd.kind() {
            FdKind::Fd(fd) => opcode::Fallocate::new(*fd, self.len),
            FdKind::Fixed(fd) => opcode::Fallocate::new(*fd, self.len),
        }
        .offset(self.offset)
        .mode(self.mode)
        .build()
    }

    fn cleanup(&mut self, _: CQEResult) {}
}

impl Singleshot for Fallocate {
    type Output = io::Result<()>;

    fn complete(self, result: CQEResult) -> Self::Output {
        result.result.map(|_| ())
    }
}
