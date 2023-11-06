use bytes::{Bytes, BytesMut};
use faser_uring::bufring::BufRing;
use faser_uring::net::UdpSocket;

mod util;

#[test]
fn test_send_recv() -> Result<(), Box<dyn std::error::Error>> {
    util::with_test_env(|| async {
        let s1 = UdpSocket::bind("127.0.0.1:0".parse()?).await?;
        let s2 = UdpSocket::bind("127.0.0.1:0".parse()?).await?;

        // Send hello to s2
        let buf = Bytes::from_static(b"hello");
        let (res, buf) = s1.send_to(buf, s2.local_addr()?).await;
        // Assert that we sent 5 bytes
        assert_eq!(buf.len(), res?);

        // Receive hello on s2
        let buf = BytesMut::with_capacity(5);
        let (res, buf) = s2.recv_from(buf).await;
        let (n, addr) = res?;
        // Assert that we received 5 bytes
        assert_eq!(5, n);
        // Assert that we received from the correct address
        assert_eq!(s1.local_addr()?, addr);
        // Assert that the message is correct
        assert_eq!(b"hello", &buf[..n]);

        Ok(())
    })
}

#[test]
fn test_send_recv_ring() -> Result<(), Box<dyn std::error::Error>> {
    util::with_test_env(|| async {
        let ring = BufRing::builder(1).buf_cnt(32).buf_len(1024 * 16).build()?;
        let s1 = UdpSocket::bind("127.0.0.1:0".parse()?).await?;
        let s2 = UdpSocket::bind("127.0.0.1:0".parse()?).await?;

        // Send hello to s2
        let buf = Bytes::from_static(b"hello");
        s1.send_to(buf, s2.local_addr()?).await.0?;

        // Receive hello on s2 using the ring
        let (buf, addr) = s2.recv_from_ring(&ring).await?;
        // Assert that we received from the correct address
        assert_eq!(s1.local_addr()?, addr);
        // Assert that the message is correct
        assert_eq!(b"hello", &buf[..5]);

        Ok(())
    })
}
