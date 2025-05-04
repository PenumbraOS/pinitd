use std::process;

use crate::error::Error;
use clap::Parser;
use pinitd_common::{
    SOCKET_ADDRESS, ServiceStatus,
    protocol::{RemoteCommand, RemoteResponse},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

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
    /// Reload a service config
    Reload { name: String },
    /// Show status of a specific service
    Status { name: String },
    /// List all known services and their status
    List,
    /// Request the daemon to shut down gracefully
    Shutdown,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let cli = Cli::parse();

    let initd_command = match cli.command {
        Commands::Start { name } => RemoteCommand::Start(name),
        Commands::Stop { name } => RemoteCommand::Stop(name),
        Commands::Restart { name } => RemoteCommand::Restart(name),
        Commands::Enable { name } => RemoteCommand::Enable(name),
        Commands::Disable { name } => RemoteCommand::Disable(name),
        Commands::Reload { name } => RemoteCommand::Reload(name),
        Commands::Status { name } => RemoteCommand::Status(name),
        Commands::List => RemoteCommand::List,
        Commands::Shutdown => RemoteCommand::Shutdown,
    };

    let mut stream = match TcpStream::connect(SOCKET_ADDRESS).await {
        Ok(stream) => stream,
        Err(_) => exit_with_message("Cannot find pinitd. Is it running?"),
    };

    let command_bytes = initd_command.encode()?;
    stream.write_all(&command_bytes).await?;
    // Indicate we're done writing. Crucial for the server to know the full message arrived if reading to end.
    stream.shutdown().await?;

    let mut response_buffer = Vec::new();
    stream.read_to_end(&mut response_buffer).await?;

    if response_buffer.is_empty() {
        exit_with_message("pinitd closed connection without sending a response")
    }

    let (response, _) = RemoteResponse::decode(&response_buffer)?;

    match response {
        RemoteResponse::Success(msg) => {
            println!("{}", msg);
            Ok(())
        }
        RemoteResponse::Error(msg) => {
            exit_with_message(&format!("Error: {msg}"));
        }
        RemoteResponse::Status(info) => {
            print_status(&[info]);
            Ok(())
        }
        RemoteResponse::List(list) => {
            if list.is_empty() {
                println!("No services configured");
            } else {
                print_status(&list);
            }
            Ok(())
        }
        RemoteResponse::ShuttingDown => {
            println!("Shutting down");
            Ok(())
        }
    }
}

fn print_status(statuses: &[ServiceStatus]) {
    println!(
        "{:<20} {:<10} {:<25} {}",
        "NAME", "ENABLED", "STATE", "CONFIG"
    );
    println!("{}", "-".repeat(80));
    for info in statuses {
        println!(
            "{:<20} {:<10} {:<25} {:?}",
            info.name,
            info.enabled.to_string(),
            info.state.to_string(),
            info.config_path
        );
    }
}

fn exit_with_message(message: &str) -> ! {
    eprintln!("{message}");
    process::exit(1);
}
