mod runtime;

use std::fs::{self, File};

use color_eyre::eyre::Context;
use const_format::formatcp;
use daemonize2::Daemonize;
use tokio::runtime::Runtime;

const DAEMON_DIR: &str = "/tmp/mcserver";
const PID_FILE_PATH: &str = formatcp!("{DAEMON_DIR}/process.pid");

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    fs::create_dir_all(DAEMON_DIR).wrap_err("Failed to create daemon directory")?;

    let stdout = File::create(formatcp!("{DAEMON_DIR}/daemon.out"))?;
    let stderr = File::create(formatcp!("{DAEMON_DIR}/daemon.err"))?;

    let daemonize = Daemonize::new()
        .pid_file(PID_FILE_PATH) // Every method except `new` and `start`
        .working_directory(DAEMON_DIR) // for default behaviour.
        .user("nobody")
        .group("nogroup")
        .umask(0o777)
        .stdout(stdout)
        .stderr(stderr)
        .privileged_action(|| "Executed before drop privileges");

    unsafe { daemonize.start() }.wrap_err("Failed to daemonize process")?;
    println!("Successfully daemonized!");

    let runtime = Runtime::new().wrap_err("Failed to create runtime")?;

    runtime.block_on(runtime::main())?;

    println!("Finished!");

    Ok(())
}
