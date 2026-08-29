#![allow(unused)]
use std::{
    borrow::Cow,
    env,
    ffi::{CString, NulError},
    fs::File,
    os::fd::{AsFd, AsRawFd, FromRawFd, IntoRawFd, OwnedFd},
    slice, thread,
};

use anyhow::Context;
use nix::{
    pty::{ForkptyResult, forkpty, openpty},
    sys::termios::{LocalFlags, OutputFlags, SetArg, tcgetattr, tcsetattr},
    unistd::{Pid, execvp, read, write},
};
use tokio::{
    io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    select,
    sync::{
        mpsc::channel,
        oneshot::{self, error::RecvError},
    },
};

async fn log_bytes(bytes: &[u8]) -> io::Result<()> {
    let contents = match str::from_utf8(bytes) {
        Ok(s) => Cow::Borrowed(s),
        Err(err) => {
            eprintln!("Failed to convert buffer to utf-8 losslessly: {err}");
            String::from_utf8_lossy(bytes)
        }
    };

    print!("Read this: {contents}");
    io::stdout().flush().await?;

    Ok(())
}

async fn parent_process(child: Pid, master: OwnedFd) -> anyhow::Result<()> {
    let mut buf = [0; 1024];

    let (tx, mut rx) = oneshot::channel::<()>();

    let master_fd = master.try_clone()?;

    let mut t_settings = tcgetattr(master_fd.as_fd())?;
    t_settings.local_flags.remove(LocalFlags::ECHO);
    tcsetattr(master.as_fd(), SetArg::TCSANOW, &t_settings);

    let handle = tokio::spawn(async move {
        loop {
            let mut buf = String::new();
            let mut stdin = BufReader::new(io::stdin());

            select! {
                biased;
                res = &mut rx => match res {
                    Ok(()) => {
                        return Ok(())
                    },
                    Err(_) => {
                        eprintln!("Unexpected error!");
                        return Ok(());
                    },
                },
                res = stdin.read_line(&mut buf) => {
                    res?;
                    write(master_fd.as_fd(), buf.as_bytes());
                }
            };
        }

        Ok::<(), anyhow::Error>(())
    });

    loop {
        match read(master.as_fd(), &mut buf) {
            Ok(count) => {
                if count > 0 {
                    log_bytes(&buf[..count]).await?;
                } else {
                    println!("Stream finished");
                    tx.send(());
                    break;
                }
            }
            Err(err) => {
                eprintln!("Failed to read from master with err: {err}");
                tx.send(());
                break;
            }
        }
    }

    handle.await?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pty = openpty(None, None)?;

    let master_fd = pty.master;
    let slave_fd = pty.slave;

    match unsafe { forkpty(None, None)? } {
        ForkptyResult::Child => {
            let mut args = env::args();
            args.next(); // remove the executable name

            let program = args.next().context("Expected an argument")?;
            println!("\x1b[0mStarting {program} now");

            let path = CString::new(program)?;

            let mut program_args = vec![path.clone()];

            args.try_for_each(|arg| -> Result<(), NulError> {
                program_args.push(CString::new(arg)?);
                Ok(())
            })?;

            execvp(&path, &program_args);
        }
        ForkptyResult::Parent { child, master } => parent_process(child, master).await?,
    };

    Ok(())
}
