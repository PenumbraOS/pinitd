#[macro_use]
extern crate log;
extern crate android_logger;

use std::{env::current_exe, path::PathBuf};

use android_31317_exploit::exploit::{ExploitKind, TriggerApp, execute, payload};
use android_logger::Config;
use clap::Parser;
use controller::Controller;
use error::Error;
use log::LevelFilter;
use worker::Worker;

mod controller;
mod error;
mod protocol;
mod socket;
mod worker;

/// Custom init system for Ai Pin
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
enum Args {
    /// Initialize pinit
    Initialize,
    /// Specializes this process as the controller, pid 2000 (shell), process
    Controller,
    /// Specializes this process as the worker, pid 1000 (shell), process
    Worker,
    /// Create custom exploit payload. NOTE: This is for internal use; rely on the pinitd for launching other processes
    BuildPayload,
}

fn main() {
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
    let executable = current_exe()?;

    match Args::parse() {
        Args::Initialize => {
            // Spawn only the controller
            init_logging_with_tag(None);
            warn!("Requesting controller start");
            let payload = init_payload(executable)?;
            Ok(execute(
                &payload,
                &TriggerApp::new(
                    "com.android.settings".into(),
                    "com.android.settings.Settings".into(),
                ),
            )?)
        }
        Args::Controller => {
            // Spawn the worker and enter work loop
            init_logging_with_tag(Some("pinit-controller".into()));
            warn!("Starting controller");
            Ok(Controller::create(executable)?)
        }
        Args::Worker => {
            init_logging_with_tag(Some("pinit-worker".into()));
            warn!("Starting worker");
            Ok(Worker::create()?)
        }
        Args::BuildPayload => {
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
            "/system/bin/sh -c \"{} controller\"",
            executable.display()
        )),
        Some("com.android.shell"),
    )?)
}
