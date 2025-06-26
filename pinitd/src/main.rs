#[macro_use]
extern crate log as base_log;
extern crate ai_pin_logger;

use std::env;

#[cfg(target_os = "android")]
use ai_pin_logger::Config;
use android_31317_exploit::{Cve31317Exploit, ExploitKind};
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
mod unit_parsing;
mod worker;
mod wrapper;
mod zygote;

/// Custom init system for Ai Pin
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
enum Args {
    /// Specializes this process as the controller process, defaults to pid 2000 (shell)
    Controller(ControllerArgs),
    /// Specializes this process as the worker process, defaults to pid 1000 (system)
    Worker(WorkerArgs),
    /// Create custom exploit payload. NOTE: This is for internal use; rely on the pinitd for launching other processes
    BuildPayload(BuildPayloadArgs),
    /// Write the correct Zygote pid fd back on spawn and perform process monitoring of the child process. NOTE: This is for internal use; rely on the pinitd for launching other processes
    #[command(name = "monitored-wrapper")]
    ZygoteSpawnWrapper(ZygoteWrapperArgs),
    /// Write the wrapper Zygote pid fd back on spawn. Used for spawning pinitd's own processes. NOTE: This is for internal use; rely on the pinitd for launching other processes
    #[command(name = "internal-wrapper")]
    InternalSpawnWrapper(InternalWrapperArgs),
}

#[derive(Parser, Debug)]
struct ControllerArgs {
    #[arg(long)]
    disable_worker: bool,

    #[arg(long)]
    is_zygote: bool,

    // TODO: system doesn't seem to implicitly have permissions to write the hidden_api_blacklist_exemptions, so it fails
    // when launching pinitd spawned services
    /// Run the controller in the pid 1000 (system) domain. If false, defaults to the pid 2000 (shell) domain
    #[arg(long)]
    use_system_domain: bool,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    _remaining_args: Vec<String>,
}

#[derive(Parser, Debug)]
struct WorkerArgs {
    /// Run the worker in the pid 2000 (shell) domain. If false, defaults to the pid 1000 (system) domain. Should be set if the controller is set to `use_system_domain`
    #[arg(long)]
    use_shell_domain: bool,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    _remaining_args: Vec<String>,
}

#[derive(Parser, Debug)]
struct BuildPayloadArgs {
    /// Run the controller in the pid 1000 (system) domain. If false, defaults to the pid 2000 (shell) domain
    #[arg(long)]
    use_system_domain: bool,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    _remaining_args: Vec<String>,
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
    #[arg(long)]
    is_zygote: bool,

    #[arg(index = 1)]
    command: String,

    #[arg(index = 2, trailing_var_arg = true, allow_hyphen_values = true)]
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
        Args::Controller(args) => {
            init_app("pinitd-controller".into());
            info!("Specializing controller");
            Ok(
                Controller::specialize(args.use_system_domain, args.disable_worker, args.is_zygote)
                    .await?,
            )
        }
        Args::Worker(args) => {
            init_app("pinitd-worker".into());
            info!("Specializing worker");
            Ok(WorkerProcess::specialize(args.use_shell_domain).await?)
        }
        Args::BuildPayload(args) => {
            init_app("pinitd-build".into());
            info!("Building init payload only");
            let payload = init_payload(args.use_system_domain)?;
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
            Wrapper::specialize_without_monitoring(args.command, args.is_zygote, true).await?;
            Ok(())
        }
    }
}

fn init_app(tag: String) {
    init_logging_with_tag(tag);
    match nix::unistd::setsid() {
        Ok(_) => info!("Successfully switched to self-owned process group"),
        Err(err) => error!("Failed to create new self-owned process group {err}"),
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

fn init_payload(use_system_domain: bool) -> Result<String> {
    let executable = env::current_exe()?;
    let executable = executable.display();

    let exploit = Cve31317Exploit::new();

    if use_system_domain {
        Ok(exploit.new_launch_payload(
            1000,
            // Enable network access (for children primarily)
            Some(3003),
            // Enable SDCard access
            Some(9997),
            "/data/",
            "com.android.settings",
            "platform:system_app:targetSdkVersion=29:complete",
            &ExploitKind::Command(format!(
                "exec {executable} internal-wrapper --is-zygote \"{executable} controller --disable-worker --use-system-domain\"",
            )),
            None,
        )?.payload)
    } else {
        Ok(exploit.new_launch_payload(
            2000,
            None,
            None,
            "/data/local/tmp/",
            "com.android.shell",
            "platform:shell:targetSdkVersion=29:complete",
            &ExploitKind::Command(format!(
                "exec {executable} internal-wrapper --is-zygote \"{executable} controller --disable-worker\"",
            )),
            Some("com.android.shell"),
        )?.payload)
    }
}
