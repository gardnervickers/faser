//! Socket operations.
//!
//! [Socket] is the core socket type
//! used by both TCP and UDP sockets
use std::io;
use std::mem::{ManuallyDrop, MaybeUninit};
use std::net::SocketAddr;
use std::os::fd::FromRawFd;
use std::pin::Pin;

use bytes::{Buf, BufMut};

use io_uring::squeue::Flags;
use io_uring::{opcode, types};
use socket2::{Domain, Protocol, SockAddr, Type};

use crate::bufring::{BufRing, BufRingBuf};
use crate::fd::FaserFd;
use crate::operation::{Operation, Singleshot};

pub(crate) struct Socket {
    fd: FaserFd,
}

impl Socket {
    pub(crate) fn from_fd(fd: FaserFd) -> Self {
        Self { fd }
    }

    pub(crate) async fn open(
        domain: Domain,
        socket_type: Type,
        protocol: Option<Protocol>,
    ) -> io::Result<Self> {
        let handle = crate::Handle::current();
        let op = OpenSocket {
            domain,
            socket_type,
            protocol,
        };
        let fd = handle.submit(op).await??;
        Ok(Self::from_fd(fd))
    }

    pub(crate) async fn bind(
        addr: SocketAddr,
        domain: Domain,
        socket_type: Type,
    ) -> io::Result<Self> {
        let addr = SockAddr::from(addr);
        let socket = Self::open(domain, socket_type, None).await?;
        let s = socket.as_socket();
        s.set_reuse_address(true)?;
        s.set_nonblocking(true)?;
        s.bind(&addr)?;
        Ok(socket)
    }

    pub(crate) async fn recv_from_ring(
        &self,
        ring: &BufRing,
    ) -> io::Result<(BufRingBuf, SocketAddr)> {
        let handle = crate::Handle::current();
        let op = RecvFromRing::new(self.fd.clone(), ring.clone());
        handle.submit(op).await?
    }

    pub(crate) async fn recv_from<B>(&self, buf: B) -> (io::Result<(usize, SocketAddr)>, B)
    where
        B: BufMut + 'static,
    {
        let handle = crate::Handle::current();
        let op = RecvFrom::new(self.fd.clone(), buf);
        handle.submit(op).await.unwrap()
    }

    pub(crate) async fn send_to<B>(&self, buf: B, addr: SocketAddr) -> (io::Result<usize>, B)
    where
        B: Buf + 'static,
    {
        let handle = crate::Handle::current();
        let op = SendTo::new(self.fd.clone(), buf, Some(addr));
        handle.submit(op).await.unwrap()
    }

    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.as_socket().local_addr()?.as_socket().unwrap())
    }

    pub(crate) fn peer_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.as_socket().peer_addr()?.as_socket().unwrap())
    }

    fn as_socket(&self) -> ManuallyDrop<socket2::Socket> {
        match self.fd.kind() {
            crate::fd::FdKind::Fd(fd) => {
                let sock = unsafe { socket2::Socket::from_raw_fd(fd.0) };
                ManuallyDrop::new(sock)
            }
            crate::fd::FdKind::Fixed(_) => unimplemented!(),
        }
    }
}

struct OpenSocket {
    domain: Domain,
    socket_type: Type,
    protocol: Option<Protocol>,
}

impl Operation for OpenSocket {
    fn configure(self: Pin<&mut Self>) -> io_uring::squeue::Entry {
        let ty: i32 = self.socket_type.into();
        let ty = ty | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC;
        io_uring::opcode::Socket::new(
            self.domain.into(),
            ty,
            self.protocol.map(Into::into).unwrap_or(0),
        )
        .build()
    }

    fn cleanup(&mut self, result: crate::operation::CQEResult) {
        if let Ok(res) = result.result {
            FaserFd::from_fd(res as i32);
        }
    }
}

impl Singleshot for OpenSocket {
    type Output = io::Result<FaserFd>;

    fn complete(self, result: crate::operation::CQEResult) -> Self::Output {
        let fd = result.result?;
        Ok(FaserFd::from_fd(fd as i32))
    }
}

struct SendTo<B> {
    fd: FaserFd,
    buf: B,
    addr: Option<SockAddr>,
    msghdr: MaybeUninit<libc::msghdr>,
    slices: MaybeUninit<[io::IoSlice<'static>; 1]>,
}

impl<B> SendTo<B>
where
    B: Buf,
{
    pub(crate) fn new(fd: FaserFd, buf: B, addr: Option<SocketAddr>) -> Self {
        let addr = addr.map(SockAddr::from);
        Self {
            fd,
            buf,
            addr,
            msghdr: MaybeUninit::zeroed(),
            slices: MaybeUninit::zeroed(),
        }
    }
}

impl<B> Operation for SendTo<B>
where
    B: Buf,
{
    fn configure(self: Pin<&mut Self>) -> io_uring::squeue::Entry {
        let this = unsafe { self.get_unchecked_mut() };

        // Initialize the slice.
        {
            let slice = io::IoSlice::new(unsafe {
                std::slice::from_raw_parts(this.buf.chunk().as_ptr(), this.buf.chunk().len())
            });
            this.slices.write([slice]);
        }

        // Next we initialize the msghdr.
        let msghdr = this.msghdr.as_mut_ptr();
        {
            let slices = unsafe { this.slices.assume_init_mut() };
            unsafe {
                (*msghdr).msg_iov = slices.as_mut_ptr() as *mut _;
                (*msghdr).msg_iovlen = slices.len() as _;
            }
        }

        // Configure the address.
        match &this.addr {
            Some(addr) => unsafe {
                (*msghdr).msg_name = addr.as_ptr() as *mut libc::c_void;
                (*msghdr).msg_namelen = addr.len() as _;
            },
            None => unsafe {
                (*msghdr).msg_name = std::ptr::null_mut();
                (*msghdr).msg_namelen = 0;
            },
        };

        let msghdr = this.msghdr.as_ptr();

        // Finally we create the operation.
        match this.fd.kind() {
            crate::fd::FdKind::Fd(fd) => opcode::SendMsg::new(types::Fd(fd.0), msghdr),
            crate::fd::FdKind::Fixed(fd) => opcode::SendMsg::new(types::Fixed(fd.0), msghdr),
        }
        .build()
    }

    fn cleanup(&mut self, _: crate::operation::CQEResult) {}
}

impl<B> Singleshot for SendTo<B>
where
    B: Buf,
{
    type Output = (io::Result<usize>, B);

    fn complete(self, result: crate::operation::CQEResult) -> Self::Output {
        (result.result.map(|v| v as usize), self.buf)
    }
}

struct RecvFrom<B> {
    fd: FaserFd,
    buf: B,
    addr: SockAddr,
    msghdr: MaybeUninit<libc::msghdr>,
    slices: MaybeUninit<[io::IoSliceMut<'static>; 1]>,
}

impl<B> RecvFrom<B>
where
    B: BufMut,
{
    pub(crate) fn new(fd: FaserFd, buf: B) -> Self {
        // Safety: We won't read from the socket addr until it's initialized.
        let addr = unsafe { SockAddr::try_init(|_, _| Ok(())) }.unwrap().1;
        Self {
            fd,
            buf,
            addr,
            msghdr: MaybeUninit::zeroed(),
            slices: MaybeUninit::zeroed(),
        }
    }
}

impl<B> Operation for RecvFrom<B>
where
    B: BufMut,
{
    fn configure(self: Pin<&mut Self>) -> io_uring::squeue::Entry {
        let this = unsafe { self.get_unchecked_mut() };

        let chunk = this.buf.chunk_mut();
        let chunk = unsafe { chunk.as_uninit_slice_mut() };
        // First we initialize the IoVecMut slice.
        this.slices.write([io::IoSliceMut::new(unsafe {
            &mut *(chunk as *mut [MaybeUninit<u8>] as *mut [u8])
        })]);
        // Safety: We just initialized the slice.
        let slices = unsafe { this.slices.assume_init_mut() };

        // Next we initialize the msghdr.
        let msghdr = this.msghdr.as_mut_ptr();
        unsafe {
            (*msghdr).msg_iov = slices.as_mut_ptr().cast();
            (*msghdr).msg_iovlen = slices.len() as _;
            (*msghdr).msg_name = this.addr.as_ptr() as *mut libc::c_void;
            (*msghdr).msg_namelen = this.addr.len() as _;
        }

        // Finally we create the operation.
        match this.fd.kind() {
            crate::fd::FdKind::Fd(fd) => opcode::RecvMsg::new(types::Fd(fd.0), msghdr),
            crate::fd::FdKind::Fixed(fd) => opcode::RecvMsg::new(types::Fixed(fd.0), msghdr),
        }
        .build()
    }

    fn cleanup(&mut self, _: crate::operation::CQEResult) {}
}

impl<B> Singleshot for RecvFrom<B>
where
    B: BufMut,
{
    type Output = (io::Result<(usize, SocketAddr)>, B);

    fn complete(self, result: crate::operation::CQEResult) -> Self::Output {
        match result.result {
            Ok(bytes_read) => {
                let addr = self.addr.as_socket().unwrap();
                let mut buf = self.buf;
                unsafe { buf.advance_mut(bytes_read as usize) };
                (Ok((bytes_read as usize, addr)), buf)
            }
            Err(err) => (Err(err), self.buf),
        }
    }
}

struct RecvFromRing {
    fd: FaserFd,
    ring: BufRing,
    addr: SockAddr,
    msghdr: MaybeUninit<libc::msghdr>,
}

impl RecvFromRing {
    pub(crate) fn new(fd: FaserFd, ring: BufRing) -> Self {
        // Safety: We won't read from the socket addr until it's initialized.
        let addr = unsafe { SockAddr::try_init(|_, _| Ok(())) }.unwrap().1;
        Self {
            fd,
            ring,
            addr,
            msghdr: MaybeUninit::zeroed(),
        }
    }
}

impl Operation for RecvFromRing {
    fn configure(self: Pin<&mut Self>) -> io_uring::squeue::Entry {
        let this = unsafe { self.get_unchecked_mut() };

        // Next we initialize the msghdr.
        let msghdr = this.msghdr.as_mut_ptr();
        unsafe {
            (*msghdr).msg_iov = std::ptr::null_mut();
            (*msghdr).msg_iovlen = 0;
            (*msghdr).msg_name = this.addr.as_ptr() as *mut libc::c_void;
            (*msghdr).msg_namelen = this.addr.len() as _;
        }

        // Finally we create the operation.
        match this.fd.kind() {
            crate::fd::FdKind::Fd(fd) => opcode::RecvMsg::new(types::Fd(fd.0), msghdr),
            crate::fd::FdKind::Fixed(fd) => opcode::RecvMsg::new(types::Fixed(fd.0), msghdr),
        }
        .buf_group(this.ring.bgid())
        .build()
        .flags(Flags::BUFFER_SELECT)
    }

    fn cleanup(&mut self, res: crate::operation::CQEResult) {
        if let Ok(n) = res.result {
            drop(self.ring.get_buf(n, res.flags));
        }
    }
}

impl Singleshot for RecvFromRing {
    type Output = io::Result<(BufRingBuf, SocketAddr)>;

    fn complete(self, result: crate::operation::CQEResult) -> Self::Output {
        let n = result.result?;
        let buf = self.ring.get_buf(n, result.flags)?;
        let addr = self.addr.as_socket().unwrap();
        Ok((buf, addr))
    }
}
