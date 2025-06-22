use std::process;

use crate::error::Result;
use clap::Parser;
use pinitd_common::{
    CONTROL_SOCKET_ADDRESS, ServiceStatus,
    protocol::{
        CLICommand, CLIResponse,
        writable::{ProtocolRead, ProtocolWrite},
    },
};
use tokio::{io::AsyncWriteExt, net::TcpStream};

mod error;

#[derive(Parser, Debug)]
#[command(author, version, about = "Control utility for the pinitd daemon", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Parser, Debug)]
enum Commands {
    /// Start a service
    Start { name: String },
    /// Stop a service
    Stop { name: String },
    /// Restart a service
    Restart { name: String },
    /// Enable a service (start on daemon boot if autostart=true)
    Enable { name: String },
    /// Disable a service (prevent autostart)
    Disable { name: String },
    /// Reload a service config from disk
    Reload { name: String },
    /// Reload all service configs from disk
    ReloadAll,
    /// Show status of a specific service
    Status { name: String },
    /// Show the current configuration of a service
    Config { name: String },
    /// List all known services and their status
    List,
    /// Request the daemon to shut down gracefully
    Shutdown,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let initd_command = match cli.command {
        Commands::Start { name } => CLICommand::Start(name),
        Commands::Stop { name } => CLICommand::Stop(name),
        Commands::Restart { name } => CLICommand::Restart(name),
        Commands::Enable { name } => CLICommand::Enable(name),
        Commands::Disable { name } => CLICommand::Disable(name),
        Commands::Reload { name } => CLICommand::Reload(name),
        Commands::ReloadAll => CLICommand::ReloadAll,
        Commands::Status { name } => CLICommand::Status(name),
        Commands::Config { name } => CLICommand::Config(name),
        Commands::List => CLICommand::List,
        Commands::Shutdown => CLICommand::Shutdown,
    };

    let mut stream = match TcpStream::connect(CONTROL_SOCKET_ADDRESS).await {
        Ok(stream) => stream,
        Err(_) => exit_with_message("Cannot find pinitd. Is it running?"),
    };

    initd_command.write(&mut stream).await?;
    // We don't need to write further
    stream.shutdown().await?;

    let response = CLIResponse::read(&mut stream).await?;

    match response {
        CLIResponse::Success(msg) => {
            println!("{}", msg);
            Ok(())
        }
        CLIResponse::Error(msg) => {
            exit_with_message(&format!("Error: {msg}"));
        }
        CLIResponse::Status(info) => {
            print_status(&[info]);
            Ok(())
        }
        CLIResponse::List(list) => {
            if list.is_empty() {
                println!("No services configured");
            } else {
                print_status(&list);
            }
            Ok(())
        }
        CLIResponse::Config(config) => {
            println!("Name: {}", config.name);
            println!("Command: {}", config.command);
            println!("Autostart: {}", config.autostart);
            println!("Restart: {:?}", config.restart);
            if let Some(nice_name) = config.nice_name {
                println!("NiceName: {nice_name}");
            }
            Ok(())
        }
        CLIResponse::ShuttingDown => {
            println!("Shutting down");
            Ok(())
        }
    }
}

fn print_status(statuses: &[ServiceStatus]) {
    println!("{:<20} {:<10} {:<25} {}", "NAME", "ENABLED", "STATE", "UID");
    println!("{}", "-".repeat(80));
    for info in statuses {
        let uid: usize = info.uid.clone().into();

        println!(
            "{:<20} {:<10} {:<25} {uid} ({:?})",
            info.name,
            info.enabled.to_string(),
            info.state.to_string(),
            info.uid
        );
    }
}

fn exit_with_message(message: &str) -> ! {
    eprintln!("{message}");
    process::exit(1);
}
