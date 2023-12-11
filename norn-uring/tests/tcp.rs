use std::io;
use std::pin::pin;

use futures_util::StreamExt;
use norn_executor::spawn;
use norn_uring::net::{TcpListener, TcpStream};

mod util;

#[test]
fn incoming_connections() -> Result<(), Box<dyn std::error::Error>> {
    util::with_test_env(|| async {
        // Bind
        let listener = TcpListener::bind("0.0.0.0:9090".parse()?, 32).await?;

        // Connect
        let handle = spawn(async {
            let stream = TcpStream::connect("0.0.0.0:9090".parse().unwrap()).await?;
            io::Result::Ok(())
        });

        let mut incoming = pin!(listener.incoming());
        let next = incoming.next().await.unwrap()?;

        Ok(())
    })
}
