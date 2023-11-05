//! Socket operations.
use std::io::{self, IoSliceMut};
use std::mem::{ManuallyDrop, MaybeUninit};
use std::net::SocketAddr;
use std::os::fd::{FromRawFd, IntoRawFd};
use std::pin::Pin;

use bytes::{BufMut, BytesMut};

use io_uring::{opcode, types};
use socket2::{Domain, Protocol, SockAddr, Type};

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
        Ok(Self { fd })
    }

    pub(crate) async fn bind(
        addr: SocketAddr,
        domain: Domain,
        socket_type: Type,
    ) -> io::Result<Self> {
        let addr = SockAddr::from(addr);
        let socket = Self::open(domain, socket_type, None).await?;
        socket.as_socket().set_reuse_address(true)?;
        socket.as_socket().set_nonblocking(true)?;
        socket.as_socket().bind(&addr)?;
        Ok(socket)
    }

    pub(crate) async fn recv(&self, buf: BytesMut) -> io::Result<(usize, BytesMut)> {
        let handle = crate::Handle::current();
        let op = Recv::new(self.fd.clone(), buf);
        let (len, buf) = handle.submit(op).await??;
        Ok((len, buf))
    }

    pub(crate) async fn recv_from(
        &self,
        buf: BytesMut,
    ) -> io::Result<((usize, SocketAddr), BytesMut)> {
        let handle = crate::Handle::current();
        let op = RecvFrom::new(self.fd.clone(), buf);
        let (len, buf) = handle.submit(op).await??;
        Ok((len, buf))
    }

    pub(crate) async fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.as_socket().local_addr()?.as_socket().unwrap())
    }

    pub(crate) async fn peer_addr(&self) -> io::Result<SocketAddr> {
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

struct RecvFrom<B> {
    fd: FaserFd,
    buf: B,
    addr: SockAddr,
    msghdr: MaybeUninit<libc::msghdr>,
    slices: MaybeUninit<[IoSliceMut<'static>; 1]>,
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
            msghdr: MaybeUninit::uninit(),
            slices: MaybeUninit::uninit(),
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
        this.slices.write([IoSliceMut::new(unsafe {
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
    type Output = io::Result<((usize, SocketAddr), B)>;

    fn complete(self, result: crate::operation::CQEResult) -> Self::Output {
        let res = result.result? as usize;
        let mut buf = self.buf;
        let addr = self.addr.as_socket().unwrap();
        unsafe { buf.advance_mut(res) };
        Ok(((res, addr), buf))
    }
}

struct Recv<B> {
    fd: FaserFd,
    buf: B,
}

impl<B> Recv<B> {
    pub(crate) fn new(fd: FaserFd, buf: B) -> Self {
        Self { fd, buf }
    }
}

impl<B> Operation for Recv<B>
where
    B: BufMut,
{
    fn configure(self: Pin<&mut Self>) -> io_uring::squeue::Entry {
        // Safety: we're not moving the buffer, so it's safe to get a pointer to it.
        let this = unsafe { self.get_unchecked_mut() };
        let chunk = this.buf.chunk_mut();
        let buf = chunk.as_mut_ptr();
        let len = chunk.len();
        match this.fd.kind() {
            crate::fd::FdKind::Fd(fd) => opcode::Recv::new(types::Fd(fd.0), buf, len as u32),
            crate::fd::FdKind::Fixed(fd) => opcode::Recv::new(types::Fixed(fd.0), buf, len as u32),
        }
        .build()
    }

    fn cleanup(&mut self, _: crate::operation::CQEResult) {}
}

impl<B> Singleshot for Recv<B>
where
    B: BufMut,
{
    type Output = io::Result<(usize, B)>;

    fn complete(self, result: crate::operation::CQEResult) -> Self::Output {
        let res = result.result?;

        let mut buf = self.buf;

        unsafe { buf.advance_mut(res as usize) };
        Ok((res as usize, buf))
    }
}

// #[cfg(test)]
// mod tests {
//     use std::net::SocketAddrV4;

//     use faser_executor::LocalExecutor;
//     use io_uring::IoUring;

//     use super::*;

//     #[test]
//     fn test_recv() -> Result<(), Box<dyn std::error::Error>> {
//         let driver = crate::Driver::new(IoUring::builder(), 32)?;
//         let mut ex = LocalExecutor::new(driver);

//         ex.block_on(async {
//             let socket = Socket::bind("0.0.0.0:5000".parse()?, Domain::IPV4, Type::DGRAM).await?;
//             println!("{:?}", socket.local_addr().await?);
//             let next = socket.recv_from(BytesMut::with_capacity(1024)).await?;
//             println!("{next:?}");
//             Ok(())
//         })
//     }
// }
