#[macro_use]
extern crate log as base_log;
extern crate ai_pin_logger;

use std::env;

#[cfg(target_os = "android")]
use ai_pin_logger::Config;
use android_31317_exploit::{DEFAULT_TRAILING_NEWLINE_COUNT, ExploitKind, launch_payload};
use base_log::LevelFilter;
use clap::Parser;
use controller::Controller;
use error::Result;
#[cfg(not(target_os = "android"))]
use simple_logger::SimpleLogger;
use uuid::Uuid;
use worker::process::WorkerProcess;
use wrapper::Wrapper;

mod controller;
mod error;
#[cfg(not(target_os = "android"))]
mod log;
mod registry;
mod state;
mod types;
mod unit_config;
mod worker;
mod wrapper;
mod zygote;

/// Custom init system for Ai Pin
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
enum Args {
    /// Specializes this process as the controller, pid 2000 (shell), process
    Controller(NoAdditionalArgs),
    /// Specializes this process as the worker, pid 1000 (system), process
    Worker(NoAdditionalArgs),
    /// Create custom exploit payload. NOTE: This is for internal use; rely on the pinitd for launching other processes
    BuildPayload(NoAdditionalArgs),
    /// Write the correct Zygote pid fd back on spawn and perform process monitoring of the child process. NOTE: This is for internal use; rely on the pinitd for launching other processes
    #[command(name = "monitored-wrapper")]
    ZygoteSpawnWrapper(ZygoteWrapperArgs),
    /// Write the wrapper Zygote pid fd back on spawn. Used for spawning pinitd's own processes. NOTE: This is for internal use; rely on the pinitd for launching other processes
    #[command(name = "internal-wrapper")]
    InternalSpawnWrapper(InternalWrapperArgs),
}

#[derive(Parser, Debug)]
struct ZygoteWrapperArgs {
    #[arg(long)]
    is_zygote: bool,

    #[arg(index = 1)]
    id: Uuid,

    #[arg(index = 2)]
    command: String,

    #[arg(index = 3, trailing_var_arg = true, allow_hyphen_values = true)]
    _remaining_args: Vec<String>,
}

#[derive(Parser, Debug)]
struct InternalWrapperArgs {
    #[arg(index = 1)]
    command: String,

    #[arg(index = 2, trailing_var_arg = true, allow_hyphen_values = true)]
    _remaining_args: Vec<String>,
}

#[derive(Parser, Debug)]
struct NoAdditionalArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    _remaining_args: Vec<String>,
}

#[tokio::main]
async fn main() {
    // Purposefully don't initialize logging until we need it, so we can specialize it for the process in question
    match run().await {
        Err(e) => {
            init_logging_with_tag("pinitd-unspecialized".into());
            error!("{e}")
        }
        _ => (),
    }
}

async fn run() -> Result<()> {
    // TODO: If we panic before initializing the logging system, we just crash without any messages
    #[cfg(target_os = "android")]
    log_panics::init();

    match Args::try_parse()? {
        Args::Controller(_) => {
            init_logging_with_tag("pinitd-controller".into());
            info!("Specializing controller");
            Ok(Controller::specialize().await?)
        }
        Args::Worker(_) => {
            init_logging_with_tag("pinitd-worker".into());
            info!("Specializing worker");
            Ok(WorkerProcess::specialize().await?)
        }
        Args::BuildPayload(_) => {
            init_logging_with_tag("pinitd-build".into());
            info!("Building init payload only");
            let payload = init_payload()?;
            // Write to stdout
            print!("{payload}");
            Ok(())
        }
        Args::ZygoteSpawnWrapper(args) => {
            init_logging_with_tag("pinitd-wrapper".into());
            Ok(Wrapper::specialize_with_monitoring(args.command, args.id, args.is_zygote).await?)
        }
        Args::InternalSpawnWrapper(args) => {
            init_logging_with_tag("pinitd-wrapper-int".into());
            Wrapper::specialize_without_monitoring(args.command, true).await?;
            Ok(())
        }
    }
}

#[cfg(target_os = "android")]
fn init_logging_with_tag(tag: String) {
    let config = Config::default().with_tag(tag);

    ai_pin_logger::init_once(config.with_max_level(LevelFilter::Trace));
}

#[cfg(not(target_os = "android"))]
fn init_logging_with_tag(tag: String) {
    use log::Logger;

    Logger::init(tag);
    // let _ = SimpleLogger::new().init();
}

fn init_payload() -> Result<String> {
    let executable = env::current_exe()?;
    let executable = executable.display();

    Ok(launch_payload(
        DEFAULT_TRAILING_NEWLINE_COUNT,
        2000,
        None,
        "/data/local/tmp/",
        "com.android.shell",
        "platform:shell:targetSdkVersion=29:complete",
        // Wrap command in sh to ensure proper permissions
        &ExploitKind::Command(format!(
            // Specifically use single quotes to preserve arguments
            // "'i=0;f=0;for a in \"\$@\";do i=\$((i+1));if [ \"\$a\" = com.android.internal.os.WrapperInit ];then eval f=\\\${\$((i+1))};break;fi;done; exec /system/bin/sh -c \"/data/local/tmp/pinitd controller & p=\\\$!; echo \\\$p >/proc/self/fd/\\\$1\" sh \"\$f\"'"
            "{executable} internal-wrapper \"{executable} controller\"",
            // "/data/local/tmp/zygote_pid_writer",
            // "/system/bin/sh -c 'nohup {} controller $0 &'",
        )),
        Some("com.android.shell"),
    )?)
}
