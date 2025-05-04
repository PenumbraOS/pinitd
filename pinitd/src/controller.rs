use std::process;

use pinitd_common::{
    create_core_directories,
    protocol::{RemoteCommand, RemoteResponse},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    signal::unix::{SignalKind, signal},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::{
    error::Error,
    registry::ServiceRegistry,
    socket::{close_socket, register_socket},
};

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

        let control_socket = register_socket().await?;

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

    async fn handle_command(
        &self,
        stream: &mut UnixStream,
        shutdown_token: CancellationToken,
    ) -> Result<RemoteResponse, Error> {
        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer).await?;

        let (command, _) = RemoteCommand::decode(&buffer)?;
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
    command: RemoteCommand,
    registry: ServiceRegistry,
    shutdown_token: CancellationToken,
) -> RemoteResponse {
    match command {
        RemoteCommand::Start(name) => match registry.service_start(name.clone()).await {
            Ok(did_start) => {
                if did_start {
                    RemoteResponse::Success(format!("Service \"{name}\" started",))
                } else {
                    RemoteResponse::Success(format!("Service \"{name}\" already running",))
                }
            }
            Err(err) => RemoteResponse::Error(format!("Failed to start service \"{name}\": {err}")),
        },
        RemoteCommand::Stop(name) => match registry.service_stop(name.clone()).await {
            Ok(_) => RemoteResponse::Success(format!("Service \"{name}\" stop initiated.")),
            Err(err) => RemoteResponse::Error(format!("Failed to stop service \"{name}\": {err}")),
        },
        RemoteCommand::Restart(name) => match registry.service_restart(name.clone()).await {
            Ok(_) => RemoteResponse::Success(format!("Service \"{name}\" restarted")),
            Err(err) => {
                RemoteResponse::Error(format!("Failed to restart service \"{name}\": {err}"))
            }
        },
        RemoteCommand::Enable(name) => match registry.service_enable(name.clone()).await {
            Ok(_) => RemoteResponse::Success(format!("Service \"{name}\" enabled")),
            Err(err) => {
                RemoteResponse::Error(format!("Failed to enable service \"{name}\": {err}"))
            }
        },
        RemoteCommand::Disable(name) => match registry.service_disable(name.clone()).await {
            Ok(_) => RemoteResponse::Success(format!("Service \"{name}\" disabled")),
            Err(err) => {
                RemoteResponse::Error(format!("Failed to disable service \"{name}\": {err}"))
            }
        },
        RemoteCommand::Reload(name) => match registry.service_reload(name.clone()).await {
            Ok(_) => RemoteResponse::Success(format!("Service \"{name}\" reloaded")),
            Err(err) => {
                RemoteResponse::Error(format!("Failed to reload service \"{name}\": {err}"))
            }
        },
        RemoteCommand::Status(name) => match registry.service_status(name).await {
            Ok(status) => RemoteResponse::Status(status),
            Err(err) => RemoteResponse::Error(err.to_string()),
        },
        RemoteCommand::List => match registry.service_list_all().await {
            Ok(list) => RemoteResponse::List(list),
            Err(err) => RemoteResponse::Error(format!("Failed to retrieve service list: {err}")),
        },
        RemoteCommand::Shutdown => {
            info!("Shutdown RemoteCommand received.");
            shutdown_token.cancel();
            RemoteResponse::ShuttingDown // Respond immediately
        }
    }
}

async fn shutdown(registry: ServiceRegistry) -> Result<(), Error> {
    info!("Initiating daemon shutdown...");
    registry.shutdown().await?;

    close_socket().await;

    info!("Goodbye");

    process::exit(0);
}
