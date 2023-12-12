use std::io;
use std::net::SocketAddr;
use std::pin::pin;

use futures_util::StreamExt;
use norn_executor::spawn;
use norn_uring::net::{TcpListener, TcpSocket};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod util;

#[test]
fn incoming_connections() -> Result<(), Box<dyn std::error::Error>> {
    util::with_test_env(|| async {
        // Bind
        let listener = TcpListener::bind("0.0.0.0:9090".parse()?, 32).await?;

        // Connect
        let handle = spawn(async {
            let _ = TcpSocket::connect("0.0.0.0:9090".parse().unwrap()).await?;
            io::Result::Ok(())
        });

        let mut incoming = pin!(listener.incoming());
        let next = incoming.next().await.unwrap()?;
        next.close().await?;
        handle.await??;

        Ok(())
    })
}

#[test]
fn echo() -> Result<(), Box<dyn std::error::Error>> {
    util::with_test_env(|| async {
        let server = EchoServer::new().await?;

        let addr = server.local_addr()?;
        spawn(server.run()).detach();
        let conn = TcpSocket::connect(addr).await?;

        // Create a 128KB buffer containing the string "hello" repeated.
        let mut buf = Vec::with_capacity(128 * 1024);
        for _ in 0..128 {
            buf.extend_from_slice(b"hello");
        }
        let (reader, writer) = conn.into_stream().owned_split();
        let mut writer = pin!(writer);
        let mut reader = pin!(reader);
        writer.write_all(&buf[..]).await?;
        writer.flush().await?;
        let mut buf2 = vec![0; buf.len()];
        println!("doing read");
        reader.read_exact(&mut buf2[..]).await?;
        assert_eq!(buf, buf2);

        Ok(())
    })
}

struct EchoServer {
    listener: TcpListener,
}

impl EchoServer {
    async fn new() -> io::Result<Self> {
        let listener = TcpListener::bind("0.0.0.0:0".parse().unwrap(), 32).await?;
        Ok(Self { listener })
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    async fn run(self) -> io::Result<()> {
        let mut incoming = pin!(self.listener.incoming());
        while let Some(stream) = incoming.next().await {
            let stream = stream?;
            spawn(async move {
                let (mut reader, mut writer) = stream.into_stream().owned_split();
                let mut reader = pin!(reader);
                let mut writer = pin!(writer);
                if let Err(err) = tokio::io::copy(&mut reader, &mut writer).await {
                    log::error!("error copying: {:?}", err)
                }
            })
            .detach();
        }
        Ok(())
    }
}
