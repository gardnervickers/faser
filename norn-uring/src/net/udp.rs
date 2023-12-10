//! UDP Protocol Socket
use std::io;
use std::net::SocketAddr;

use socket2::{Domain, Type};

use crate::buf::{StableBuf, StableBufMut};
use crate::bufring::{BufRing, BufRingBuf};
use crate::net::socket;

/// A UDP socket.
///
/// After creating a `UdpSocket` by [`bind`]ing it to a socket address, data can be
/// [sent to] and [received from] any other socket address.
pub struct UdpSocket {
    inner: socket::Socket,
}

impl std::fmt::Debug for UdpSocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UdpSocket").finish()
    }
}

impl UdpSocket {
    /// Creates a UDP socket from the given address.
    pub async fn bind(addr: SocketAddr) -> io::Result<UdpSocket> {
        let inner = socket::Socket::bind(addr, Domain::for_address(addr), Type::DGRAM).await?;
        Ok(UdpSocket { inner })
    }

    /// Returns the socket address that this socket was created from.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    /// Returns the socket address of the remote peer.
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.inner.peer_addr()
    }

    /// Sends a single datagram message on the socket to the given address.
    ///
    /// On success, returns the number of bytes written.
    ///
    /// This takes ownership of the buffer provided and will return it back
    /// once the operation has completed.
    pub async fn send_to<B>(&self, buf: B, addr: SocketAddr) -> (io::Result<usize>, B)
    where
        B: StableBuf + 'static,
    {
        self.inner.send_to(buf, addr).await
    }

    /// Receives a single datagram message on the socket. On success, returns the number
    /// of bytes read and the origin.
    ///
    /// This must be called with a buf of sufficient size to hold the message. If a message
    /// is too long to fit in the supplied, buffer, excess bytes may be discarded.
    pub async fn recv_from<B>(&self, buf: B) -> (io::Result<(usize, SocketAddr)>, B)
    where
        B: StableBufMut + 'static,
    {
        self.inner.recv_from(buf).await
    }

    /// Sends a single datagram message on the socket to the given address using a buffer
    /// from the given ring.
    ///
    /// The buffer ring used must contain buffers of sufficient size to hold the message. If
    /// a message is too long to fit in the supplied, buffer, excess bytes may be discarded.
    pub async fn recv_from_ring(&self, ring: &BufRing) -> io::Result<(BufRingBuf, SocketAddr)> {
        self.inner.recv_from_ring(ring).await
    }

    /// Close the socket.
    ///
    /// This will wait for all pending operations to complete before closing the socket.
    pub async fn close(self) -> io::Result<()> {
        self.inner.close().await
    }
}
