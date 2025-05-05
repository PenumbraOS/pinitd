use std::process;

use pinitd_common::{
    SOCKET_ADDRESS,
    bincode::Bincodable,
    create_core_directories,
    protocol::{CLICommand, CLIResponse},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    signal::unix::{SignalKind, signal},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::{error::Error, registry::ServiceRegistry};

#[derive(Clone)]
pub struct Controller {
    registry: ServiceRegistry,
}

impl Controller {
    pub async fn create() -> Result<(), Error> {
        create_core_directories();

        let registry = ServiceRegistry::load().await?;
        registry.autostart_all().await?;

        let controller = Controller { registry };

        let shutdown_token = CancellationToken::new();
        let mut should_shutdown = setup_signal_watchers(shutdown_token.clone())?;

        let control_socket = TcpListener::bind(&SOCKET_ADDRESS).await?;

        info!("Controller started");

        loop {
            tokio::select! {
                result = control_socket.accept() => {
                    match result {
                        Ok((mut stream, _)) => {
                            info!("Accepted new client connection");
                            let controller_clone = controller.clone();
                            let shutdown_token_clone = shutdown_token.clone();
                            tokio::spawn(async move {
                                match controller_clone.handle_command(&mut stream, shutdown_token_clone).await {
                                    Ok(response) => {
                                        match response.encode() {
                                            Ok(data) => {
                                                let _ = stream.write_all(&data).await;
                                                let _ = stream.shutdown().await;
                                            },
                                            Err(err) => error!("Error responding to client: {err:?}"),
                                        }
                                    },
                                    Err(err) => error!("Error handling client: {err:?}"),
                                }
                            });
                        }
                        Err(e) => {
                            error!("Error accepting control connection: {}", e);
                        }
                    }
                }
                _ = &mut should_shutdown => {
                    info!("Signal shutdown received");
                    break;
                }
            }
        }

        shutdown(controller.registry).await?;

        info!("After shutdown");

        Ok(())
    }

    async fn handle_command<T>(
        &self,
        stream: &mut T,
        shutdown_token: CancellationToken,
    ) -> Result<CLIResponse, Error>
    where
        T: AsyncRead + Unpin,
    {
        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer).await?;

        let (command, _) = CLICommand::decode(&buffer)?;
        info!("Received RemoteCommand: {:?}", command);

        let response = process_remote_command(command, self.registry.clone(), shutdown_token).await;

        Ok(response)
    }
}

fn setup_signal_watchers(shutdown_token: CancellationToken) -> Result<JoinHandle<()>, Error> {
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    let shutdown_signal_task = tokio::spawn(async move {
        tokio::select! {
            _ = sigterm.recv() => {
                info!("Received SIGTERM, initiating shutdown...");
            },
            _ = sigint.recv() => {
                 info!("Received SIGINT (Ctrl+C), initiating shutdown...");
            },
            _ = shutdown_token.cancelled() => {
                info!("Received shutdown command, initiating shutdown...");
            }
        }
    });

    Ok(shutdown_signal_task)
}

async fn process_remote_command(
    command: CLICommand,
    registry: ServiceRegistry,
    shutdown_token: CancellationToken,
) -> CLIResponse {
    match command {
        CLICommand::Start(name) => match registry.service_start(name.clone()).await {
            Ok(did_start) => {
                if did_start {
                    CLIResponse::Success(format!("Service \"{name}\" started",))
                } else {
                    CLIResponse::Success(format!("Service \"{name}\" already running",))
                }
            }
            Err(err) => CLIResponse::Error(format!("Failed to start service \"{name}\": {err}")),
        },
        CLICommand::Stop(name) => match registry.service_stop(name.clone()).await {
            Ok(_) => CLIResponse::Success(format!("Service \"{name}\" stop initiated.")),
            Err(err) => CLIResponse::Error(format!("Failed to stop service \"{name}\": {err}")),
        },
        CLICommand::Restart(name) => match registry.service_restart(name.clone()).await {
            Ok(_) => CLIResponse::Success(format!("Service \"{name}\" restarted")),
            Err(err) => CLIResponse::Error(format!("Failed to restart service \"{name}\": {err}")),
        },
        CLICommand::Enable(name) => match registry.service_enable(name.clone()).await {
            Ok(_) => CLIResponse::Success(format!("Service \"{name}\" enabled")),
            Err(err) => CLIResponse::Error(format!("Failed to enable service \"{name}\": {err}")),
        },
        CLICommand::Disable(name) => match registry.service_disable(name.clone()).await {
            Ok(_) => CLIResponse::Success(format!("Service \"{name}\" disabled")),
            Err(err) => CLIResponse::Error(format!("Failed to disable service \"{name}\": {err}")),
        },
        CLICommand::Reload(name) => match registry.service_reload(name.clone()).await {
            Ok(_) => CLIResponse::Success(format!("Service \"{name}\" reloaded")),
            Err(err) => CLIResponse::Error(format!("Failed to reload service \"{name}\": {err}")),
        },
        CLICommand::Status(name) => match registry.service_status(name).await {
            Ok(status) => CLIResponse::Status(status),
            Err(err) => CLIResponse::Error(err.to_string()),
        },
        CLICommand::List => match registry.service_list_all().await {
            Ok(list) => CLIResponse::List(list),
            Err(err) => CLIResponse::Error(format!("Failed to retrieve service list: {err}")),
        },
        CLICommand::Shutdown => {
            info!("Shutdown RemoteCommand received.");
            shutdown_token.cancel();
            CLIResponse::ShuttingDown // Respond immediately
        }
    }
}

async fn shutdown(registry: ServiceRegistry) -> Result<(), Error> {
    info!("Initiating daemon shutdown...");
    registry.shutdown().await?;

    info!("Goodbye");

    process::exit(0);
}
