use std::io;
use std::mem::ManuallyDrop;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{ready, Context, Poll};

use futures_core::Stream;
use socket2::{Domain, Type};

use crate::buf::{StableBuf, StableBufMut};
use crate::net::socket;
use crate::operation::Op;

use super::socket::Accept;

/// A TCP listener.
///
/// A TcpListener can be used to accept incoming TCP connections.
pub struct TcpListener {
    inner: socket::Socket,
}

impl std::fmt::Debug for TcpListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TcpListener").finish()
    }
}

impl TcpListener {
    /// Creates a TCP listener bound to the specified address.
    pub async fn bind(addr: SocketAddr, backlog: u32) -> io::Result<TcpListener> {
        let inner = socket::Socket::bind(addr, Domain::for_address(addr), Type::STREAM).await?;
        inner.listen(backlog)?;
        Ok(TcpListener { inner })
    }

    /// Returns the local address that this listener is bound to.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    /// Accepts a new incoming connection to this listener.
    pub async fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        let (socket, addr) = self.inner.accept().await?;
        Ok((TcpStream { socket }, addr))
    }

    /// Returns a stream of incoming connections.
    pub fn incoming(&self) -> Incoming<'_> {
        Incoming {
            listener: &self.inner,
            current: None,
        }
    }

    /// Closes the listener.
    pub async fn close(self) -> io::Result<()> {
        self.inner.close().await
    }
}

pin_project_lite::pin_project! {
    pub struct Incoming<'a> {
        listener: &'a socket::Socket,
        #[pin]
        current: Option<Op<Accept<true>>>,
    }
}

impl<'a> Stream for Incoming<'a> {
    type Item = io::Result<TcpStream>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        loop {
            if let Some(current) = this.current.as_mut().as_pin_mut() {
                match ready!(current.poll_next(cx)) {
                    Some(Err(err)) => {
                        this.current.set(None);
                        return Poll::Ready(Some(Err(err)));
                    }

                    Some(Ok(socket)) => {
                        let socket = socket::Socket::from_fd(socket);
                        return Poll::Ready(Some(Ok(TcpStream { socket })));
                    }
                    None => {
                        this.current.set(None);
                        return Poll::Ready(None);
                    }
                }
            }
            this.current.set(Some(this.listener.accept_multi()));
        }
    }
}

/// A TCP stream.
pub struct TcpStream {
    socket: socket::Socket,
}

impl std::fmt::Debug for TcpStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TcpStream").finish()
    }
}

impl TcpStream {
    /// Creates a TCP connection to the specified address.
    pub async fn connect(addr: SocketAddr) -> io::Result<TcpStream> {
        let domain = Domain::for_address(addr);
        let socket_type = Type::STREAM;
        let protocol = None;
        let socket = socket::Socket::open(domain, socket_type, protocol).await?;
        socket.connect(addr).await?;
        Ok(TcpStream { socket })
    }

    /// Returns the local address that this stream is bound to.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// Returns the remote address that this stream is connected to.
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.socket.peer_addr()
    }

    /// Shuts down the read, write, or both halves of this connection.
    pub async fn shutdown(&self, how: std::net::Shutdown) -> io::Result<()> {
        self.socket.shutdown(how).await
    }

    /// Split the stream into a reader and a writer.
    pub fn split(&self) -> (TcpStreamReader, TcpStreamWriter) {
        let reader = TcpStreamReader {
            inner: ReadyStream::new(self.socket.clone()),
        };
        let writer = TcpStreamWriter {
            inner: ReadyStream::new(self.socket.clone()),
        };
        (reader, writer)
    }

    /// Close the socket.
    pub async fn close(self) -> io::Result<()> {
        self.socket.close().await
    }
}

impl crate::io::AsyncReadOwned for TcpStream {
    type ReadFuture<'a, B> = Op<socket::Recv<B>>
    where
        B: StableBufMut,
        Self: 'a;

    fn read<B: StableBufMut>(&mut self, buf: B) -> Self::ReadFuture<'_, B> {
        self.socket.recv(buf)
    }
}

impl crate::io::AsyncWriteOwned for TcpStream {
    type WriteFuture<'a, B> = Op<socket::Send<B>>
    where
        B: StableBuf,
        Self: 'a;

    fn write<B: StableBuf>(&mut self, buf: B) -> Self::WriteFuture<'static, B> {
        self.socket.send(buf)
    }
}

pin_project_lite::pin_project! {
    pub struct TcpStreamReader {
        #[pin]
        inner: ReadyStream,
    }
}

pin_project_lite::pin_project! {
    pub struct TcpStreamWriter {
        #[pin]
        inner: ReadyStream,
    }
}

impl tokio::io::AsyncRead for TcpStreamReader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.project();
        let n = ready!(this.inner.poll_op(
            cx,
            |sock| unsafe { sock.recv(buf.unfilled_mut()) },
            socket::READ_FLAGS as u32,
        ))?;
        buf.advance(n);
        Poll::Ready(Ok(()))
    }
}

impl tokio::io::AsyncWrite for TcpStreamWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.project();
        let n = ready!(this
            .inner
            .poll_op(cx, |sock| sock.send(buf), socket::WRITE_FLAGS as u32))?;
        Poll::Ready(Ok(n))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        use std::io::Write;
        let this = self.project();
        ready!(this
            .inner
            .poll_op(cx, |mut sock| sock.flush(), socket::WRITE_FLAGS as u32))?;
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let this = self.project();
        ready!(this.inner.poll_op(
            cx,
            |sock| sock.shutdown(std::net::Shutdown::Write),
            socket::WRITE_FLAGS as u32
        ))?;
        Poll::Ready(Ok(()))
    }
}

pin_project_lite::pin_project! {
    struct ReadyStream {
        inner: socket::Socket,
        #[pin]
        armed: Option<Op<socket::Poll<true>>>,
        notified: bool,
    }
}

impl ReadyStream {
    fn new(inner: socket::Socket) -> Self {
        Self {
            inner,
            armed: None,
            notified: true,
        }
    }

    fn poll_op<U>(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut f: impl FnMut(ManuallyDrop<socket2::Socket>) -> io::Result<U>,
        flags: u32,
    ) -> Poll<io::Result<U>> {
        loop {
            log::trace!("poll_op");
            ready!(self.as_mut().poll_ready(cx, flags))?;
            log::trace!("poll_op.ready");
            let this = self.as_mut().project();
            let sock = this.inner.as_socket();
            match f(sock) {
                Ok(res) => {
                    log::trace!("poll_op.success");
                    return Poll::Ready(Ok(res));
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    log::trace!("poll_op.would_block");
                    *this.notified = false;
                    ready!(self.as_mut().poll_ready(cx, flags))?;
                    continue;
                }
                Err(err) => {
                    log::trace!("poll_op.err");
                    return Poll::Ready(Err(err));
                }
            }
        }
    }
}

impl ReadyStream {
    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>, flags: u32) -> Poll<io::Result<()>> {
        let mut this = self.project();
        loop {
            if *this.notified {
                // If the notified bit is set, return immediately.
                return Poll::Ready(Ok(()));
            }
            if let Some(armed) = this.armed.as_mut().as_pin_mut() {
                if let Some(res) = ready!(armed.poll_next(cx)) {
                    res?;
                    *this.notified = true;
                } else {
                    this.armed.set(None);
                }
            } else {
                this.armed.set(Some(this.inner.poll_ready(flags)));
            }
        }
    }
}
