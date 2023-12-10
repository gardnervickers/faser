use std::io;
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
                    Some(Ok(Err(err))) => {
                        this.current.set(None);
                        return Poll::Ready(Some(Err(err)));
                    }
                    Some(Ok(Ok(socket))) => {
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
        let socket = socket::Socket::connect(addr).await?;
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

    /// Send the given buffer to the connected peer.
    pub async fn send<B>(&self, buf: B) -> (io::Result<usize>, B)
    where
        B: StableBuf + 'static,
    {
        self.socket.send(buf).await
    }

    /// Receive a buffer from the connected peer.
    pub async fn recv<B>(&self, buf: B) -> (io::Result<usize>, B)
    where
        B: StableBufMut + 'static,
    {
        self.socket.recv(buf).await
    }
}
