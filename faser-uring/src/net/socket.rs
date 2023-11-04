//! Socket operations.
use std::io;
use std::pin::Pin;

use bytes::BytesMut;
use io_uring::{opcode, types};

use crate::fd::FaserFd;
use crate::operation::{Operation, Singleshot};

pub(crate) struct Socket {
    fd: FaserFd,
}

impl Socket {
    pub(crate) fn from_fd(fd: FaserFd) -> Self {
        Self { fd }
    }

    pub(crate) async fn open(domain: i32, socket_type: i32, protocol: i32) -> io::Result<Self> {
        let handle = crate::Handle::current();
        let op = OpenSocket {
            domain,
            socket_type,
            protocol,
        };
        let fd = handle.submit(op).await??;
        Ok(Self { fd })
    }

    pub(crate) async fn recv(&self, buf: BytesMut) -> io::Result<(usize, BytesMut)> {
        let handle = crate::Handle::current();
        let op = Recv::new(self.fd.clone(), buf);
        let (len, buf) = handle.submit(op).await??;
        Ok((len, buf))
    }
}

struct OpenSocket {
    domain: i32,
    socket_type: i32,
    protocol: i32,
}

impl Operation for OpenSocket {
    fn configure(self: Pin<&mut Self>) -> io_uring::squeue::Entry {
        io_uring::opcode::Socket::new(self.domain, self.socket_type, self.protocol).build()
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

struct Recv {
    fd: FaserFd,
    buf: BytesMut,
}

impl Recv {
    pub(crate) fn new(fd: FaserFd, buf: BytesMut) -> Self {
        Self { fd, buf }
    }
}

impl Operation for Recv {
    fn configure(self: Pin<&mut Self>) -> io_uring::squeue::Entry {
        let this = self.get_mut();
        let buf = this.buf.as_mut_ptr();
        let len = this.buf.capacity();
        match this.fd.kind() {
            crate::fd::FdKind::Fd(fd) => opcode::Recv::new(types::Fd(fd.0), buf, len as u32),
            crate::fd::FdKind::Fixed(fd) => opcode::Recv::new(types::Fixed(fd.0), buf, len as u32),
        }
        .build()
    }

    fn cleanup(&mut self, _: crate::operation::CQEResult) {}
}

impl Singleshot for Recv {
    type Output = io::Result<(usize, BytesMut)>;

    fn complete(self, result: crate::operation::CQEResult) -> Self::Output {
        let res = result.result?;

        let mut buf = self.buf;
        unsafe { buf.set_len(res as usize) };
        Ok((res as usize, buf))
    }
}

// #[cfg(test)]
// mod tests {
//     use io_uring::IoUring;

//     use super::*;

//     #[test]
//     fn foo() -> Result<(), Box<dyn std::error::Error>> {
//         let driver = crate::Driver::new(IoUring::builder(), 32)?;
//         let mut ex = faser_executor::LocalExecutor::new(driver);

//         ex.block_on(async {
//             let socket = Socket::open(
//                 libc::AF_INET,
//                 libc::SOCK_DGRAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
//                 0,
//             )
//             .await?;

//             let (read, buf) = socket.recv(BytesMut::with_capacity(1024)).await?;

//             Ok(())
//         })
//     }
// }
