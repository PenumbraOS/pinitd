use crate::error::Error;
use clap::Parser;
use pinitd_common::{
    SOCKET_PATH, ServiceStatus,
    protocol::{RemoteCommand, RemoteResponse},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

mod error;

#[derive(Parser, Debug)]
#[command(author, version, about = "Control utility for the initd daemon", long_about = None)]
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
    /// Enable a service (start on daemon boot if autostart=true)
    Enable { name: String },
    /// Disable a service (prevent autostart)
    Disable { name: String },
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
        Commands::Enable { name } => RemoteCommand::Enable(name),
        Commands::Disable { name } => RemoteCommand::Disable(name),
        Commands::Status { name } => RemoteCommand::Status(name),
        Commands::List => RemoteCommand::List,
        Commands::Shutdown => RemoteCommand::Shutdown,
    };

    // --- Connect to Daemon ---
    let mut stream = UnixStream::connect(SOCKET_PATH).await?;

    let command_bytes = initd_command.encode()?;
    stream.write_all(&command_bytes).await?;
    // Indicate we're done writing. Crucial for the server to know the full message arrived if reading to end.
    stream.shutdown().await?;

    let mut response_buffer = Vec::new();
    stream.read_to_end(&mut response_buffer).await?;

    if response_buffer.is_empty() {
        return Err(Error::Unknown(
            "Daemon closed connection without sending a response".to_string(),
        ));
    }

    let (response, _) = RemoteResponse::decode(&response_buffer)?;

    // --- Process Response ---
    match response {
        RemoteResponse::Success(msg) => {
            println!("{}", msg);
            Ok(())
        }
        RemoteResponse::Error(msg) => {
            eprintln!("Error: {}", msg);
            Err(Error::Unknown("Daemon reported error".to_string()))
        }
        RemoteResponse::Status(info) => {
            print_status(&[info]);
            Ok(())
        }
        RemoteResponse::List(list) => {
            if list.is_empty() {
                println!("No services configured.");
            } else {
                print_status(&list);
            }
            Ok(())
        }
        RemoteResponse::ShuttingDown => {
            println!("Daemon shutdown initiated.");
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
