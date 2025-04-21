#[macro_use]
extern crate log;
extern crate android_logger;

use std::env::current_exe;

use android_31317_exploit::exploit::{ExploitKind, execute};
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
            Ok(execute(
                2000,
                "/data/local/tmp/",
                "com.android.shell",
                "platform:shell:targetSdkVersion=29:complete",
                ExploitKind::Command(format!("{} controller", executable.display())),
                Some("com.android.shell"),
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
