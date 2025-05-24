#[macro_use]
extern crate log as base_log;
extern crate ai_pin_logger;

use std::env;

#[cfg(target_os = "android")]
use ai_pin_logger::Config;
use android_31317_exploit::{ExploitKind, payload};
use base_log::LevelFilter;
use clap::Parser;
use controller::Controller;
use error::Result;
#[cfg(not(target_os = "android"))]
use simple_logger::SimpleLogger;
use worker::process::WorkerProcess;
use zygote::extract_and_write_fd;

mod controller;
mod error;
#[cfg(not(target_os = "android"))]
mod log;
mod registry;
mod state;
mod types;
mod unit_config;
mod worker;
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
    log_panics::init();

    #[cfg(target_os = "android")]
    if let Err(error) = extract_and_write_fd() {
        error!("fd error: {error}");
    }

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

    Ok(payload(
        2000,
        "/data/local/tmp/",
        "com.android.shell",
        "platform:shell:targetSdkVersion=29:complete",
        // Wrap command in sh to ensure proper permissions
        &ExploitKind::Command(format!(
            // Specifically use single quotes to preserve arguments
            // "'i=0;f=0;for a in \"\$@\";do i=\$((i+1));if [ \"\$a\" = com.android.internal.os.WrapperInit ];then eval f=\\\${\$((i+1))};break;fi;done; exec /system/bin/sh -c \"/data/local/tmp/pinitd controller & p=\\\$!; echo \\\$p >/proc/self/fd/\\\$1\" sh \"\$f\"'"
            "{} controller",
            // "/data/local/tmp/zygote_pid_writer",
            // "/system/bin/sh -c 'nohup {} controller $0 &'",
            executable.display()
        )),
        Some("com.android.shell"),
    )?)
}
