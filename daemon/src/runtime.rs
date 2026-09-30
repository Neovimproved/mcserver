use const_format::formatcp;
use tokio::{io::AsyncReadExt, net::TcpListener, select};

const MCSERVER_DAEMON_PORT: u16 = 56608;

pub async fn main() -> color_eyre::Result<()> {
    println!("Doing things...");

    let mut listener = TcpListener::bind(formatcp!("127.0.0.1:{MCSERVER_DAEMON_PORT}")).await?;

    println!("Bound to address!");

    let (mut stream, addr) = listener.accept().await?;

    println!("Stream: {stream:?}\nAddress: {addr}");

    let (mut reader, _writer) = stream.split();

    loop {
        let mut buf = [0u8; 1024];

        let count = reader.read(&mut buf).await?;

        if count == 0 {
            break;
        }

        println!("Read the following: ")
    }

    let mut fut = async {};

    select! {
        _ = fut => {
            println!("Nice")
        }
    }

    Ok(())
}
