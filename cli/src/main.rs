use std::env;

use color_eyre::eyre::eyre;
use mcserver_core::*;
use tokio::{
    io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    select,
};

async fn request_server_start(mut writer: impl AsyncWriteExt + Unpin) -> color_eyre::Result<()> {
    let mut path = env::home_dir().ok_or(io::Error::new(
        io::ErrorKind::NotFound,
        "could not find home directory",
    ))?;

    path.extend(&[
        "Dev",
        "example-terminal-app",
        "target",
        "debug",
        "example-terminal-app",
    ]);

    writer
        .write_all(&bitcode::encode(&ClientMessage::Start {
            name: path.into_string().map_err(|path_buf| {
                eyre!("Failed to convert {} into string", path_buf.display())
            })?,
            flags: vec![],
        }))
        .await?;

    Ok(())
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let mut stream = UnixStream::connect(SOCKET_PATH).await?;

    let (mut reader, mut writer) = stream.split();

    request_server_start(&mut writer).await?;

    let mut stdin_reader = BufReader::new(io::stdin());

    loop {
        let mut server_message_buf = [0u8; 1024];
        let mut string_buf = String::with_capacity(1024);

        select! {
            res = stdin_reader.read_line(&mut string_buf) => {
                res?;

                string_buf.pop(); // remove the newline

                writer.write_all(&bitcode::encode(
                        &ClientMessage::ProgramInput(string_buf.into_bytes())
                )).await?;
            }
            res = reader.read(&mut server_message_buf)  => {
                let count = res?;

                if count == 0 {
                    break;
                }

                let decoded: ServerMessage = bitcode::decode(&server_message_buf[..count])?;

                println!("Read the following bytes ({count}): {}", decoded);
            }
        }
    }

    println!("Done");

    Ok(())
}
