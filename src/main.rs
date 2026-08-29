use std::{
    borrow::Cow,
    env,
    ffi::CString,
    io::{self, Write},
    os::fd::{AsFd, OwnedFd},
    sync::mpsc::{TryRecvError, channel},
    thread,
};

use anyhow::Context;
use nix::{
    errno::Errno,
    libc,
    pty::{ForkptyResult, forkpty},
    sys::{
        termios::{LocalFlags, SetArg, tcgetattr, tcsetattr},
        wait::{WaitStatus, waitpid},
    },
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

    let writer_master_fd = master.try_clone()?;

    let mut t_settings = tcgetattr(writer_master_fd.as_fd())?;
    t_settings.local_flags.remove(LocalFlags::ECHO);
    tcsetattr(master.as_fd(), SetArg::TCSANOW, &t_settings)?;

    let handle = thread::spawn(move || -> anyhow::Result<()> {
        loop {
            use std::os::fd::AsRawFd;

            let mut pfd = libc::pollfd {
                fd: std::io::stdin().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };

            let ret = unsafe { libc::poll(&mut pfd, 1, 0) };

            if ret > 0 && (pfd.revents & libc::POLLIN) != 0 {
                let mut buf = String::new();
                io::stdin().read_line(&mut buf)?;

                if write(writer_master_fd.as_fd(), buf.as_bytes())? == 0 {
                    eprintln!("Failed to write");
                    return Ok(());
                }
            } else {
                match rx.try_recv() {
                    Ok(()) => break,
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        eprintln!("Unexpected disconnect");
                        break;
                    }
                };
            }
        }

        Ok::<(), anyhow::Error>(())
    });

    loop {
        match read(master.as_fd(), &mut buf) {
            Ok(count) => {
                if count > 0 {
                    log_bytes(&buf[..count])?;
                } else {
                    println!("Stream finished");
                    tx.send(())?;
                    break;
                }
            }
            Err(err) => {
                if err != Errno::EIO {
                    eprintln!("Failed to read from master with err: {err}");
                }
                tx.send(())?;
                break;
            }
        }
    }

    handle.join().expect("Failed to join handle")?;

    match waitpid(child, None)? {
        WaitStatus::Exited(_, _) => {} // graceful exit
        WaitStatus::Signaled(_, signal, _) => eprintln!("Child killed by signal {signal:?}"),
        status => println!("Child status: {status:?}"),
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    match unsafe { forkpty(None, None)? } {
        ForkptyResult::Child => {
            let mut args = env::args();
            args.next(); // remove the executable name

            let program = args.next().context("Expected an argument")?;

            let path = CString::new(program)?;

            let mut program_args = vec![path.clone()];

            for arg in args {
                program_args.push(CString::new(arg)?);
            }

            program_args.iter().for_each(|arg| {
                print!("{arg:?} ");
            });

            execvp(&path, &program_args)?;
        }
        ForkptyResult::Parent { child, master } => parent_process(child, master)?,
    };

    Ok(())
}
