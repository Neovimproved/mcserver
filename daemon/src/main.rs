mod runtime;

use std::{
    env,
    fs::{self, File},
};

use color_eyre::eyre::Context;
use const_format::formatcp;
use daemonize2::Daemonize;
use mcserver_core::*;
use tokio::runtime::Runtime;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    fs::create_dir_all(TEMP_DIR).wrap_err("Failed to create daemon directory")?;

    if let Some(arg) = env::args().nth(1)
        && arg == "no"
    {
        // no daemonize
        Runtime::new()?.block_on(runtime::main())?;
        return Ok(());
    }

    let stdout = File::create(formatcp!("{TEMP_DIR}/daemon.out"))?;
    let stderr = File::create(formatcp!("{TEMP_DIR}/daemon.err"))?;

    let daemonize = Daemonize::new()
        .pid_file(PID_FILE_PATH) // Every method except `new` and `start`
        .working_directory(TEMP_DIR) // for default behaviour.
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
