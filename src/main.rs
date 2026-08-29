#![allow(unused)]
use std::{
    borrow::Cow,
    env,
    ffi::{CString, NulError},
    fs::File,
    io::{self, BufRead, BufWriter, Write},
    os::fd::{AsFd, AsRawFd, FromRawFd, IntoRawFd, OwnedFd},
    slice,
    sync::mpsc::{TryRecvError, channel},
    thread,
};

use anyhow::Context;
use nix::{
    pty::{ForkptyResult, forkpty, openpty},
    sys::termios::tcgetattr,
    unistd::{Pid, execvp, read, write},
};

fn log_bytes(bytes: &[u8]) -> io::Result<()> {
    let contents = match str::from_utf8(bytes) {
        Ok(s) => Cow::Borrowed(s),
        Err(err) => {
            eprintln!("Failed to convert buffer to utf-8 losslessly: {err}");
            String::from_utf8_lossy(bytes)
        }
    };

    print!("Read this: {contents}");
    io::stdout().flush()?;

    Ok(())
}

fn parent_process(child: Pid, master: OwnedFd) -> anyhow::Result<()> {
    let mut buf = [0; 1024];

    let (tx, rx) = channel();

    let master_fd = master.try_clone()?;

    let handle = thread::spawn(move || -> anyhow::Result<()> {
        let mut t_settings = tcgetattr(master_fd.as_fd())?;

        while match rx.try_recv() {
            Ok(()) => false,
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                eprintln!("Unexpected disconnect");
                false
            }
        } {
            let mut buf = String::new();
            io::stdin().read_line(&mut buf);

            write(master_fd.as_fd(), buf.as_bytes());
        }

        Ok(())
    });

    loop {
        match read(master.as_fd(), &mut buf) {
            Ok(count) => {
                if count > 0 {
                    log_bytes(&buf[..count])?;
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

    handle.join().expect("Failed to join handle");

    Ok(())
}

fn main() -> anyhow::Result<()> {
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
        ForkptyResult::Parent { child, master } => parent_process(child, master)?,
    };

    Ok(())
}
