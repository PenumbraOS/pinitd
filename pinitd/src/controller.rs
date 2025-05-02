use std::process;

use pinitd_common::protocol::{RemoteCommand, RemoteResponse};
use tokio::{
    io::AsyncReadExt,
    net::UnixStream,
    signal::unix::{SignalKind, signal},
    task::JoinHandle,
};

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
        let registry = ServiceRegistry::load().await?;
        registry.autostart_all().await?;

        let controller = Controller { registry };

        let mut should_shutdown = setup_signal_watchers()?;

        let control_socket = register_socket().await?;

        info!("Controller started");
        let mut did_signal = false;

        loop {
            tokio::select! {
                result = control_socket.accept() => {
                    match result {
                        Ok((stream, _addr)) => {
                            info!("Accepted new client connection");
                            let controller_clone = controller.clone();
                            tokio::spawn(async move {
                                match controller_clone.handle_command(stream).await {
                                    // TODO: Handle sending response
                                    Ok(_) => todo!(),
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
                    did_signal = true;
                    break;
                }
            }
        }

        shutdown(controller.registry, did_signal).await?;

        Ok(())
    }

    async fn handle_command(&self, mut stream: UnixStream) -> Result<RemoteResponse, Error> {
        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer).await?;

        let (command, _): (RemoteCommand, usize) =
            bincode::serde::decode_from_slice(&buffer, bincode::config::standard())?;
        info!("Received RemoteCommand: {:?}", command);

        let response = process_remote_command(command, self.registry.clone()).await;

        Ok(response)
    }
}

fn setup_signal_watchers() -> Result<JoinHandle<()>, Error> {
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
        }
    });

    Ok(shutdown_signal_task)
}

async fn process_remote_command(
    command: RemoteCommand,
    registry: ServiceRegistry,
) -> RemoteResponse {
    match command {
        RemoteCommand::Start(name) => {
            match registry.spawn_and_monitor_service(name.clone()).await {
                Ok(_) => RemoteResponse::Success(format!(
                    "Service \"{name}\" started or already running.",
                )),
                Err(err) => {
                    RemoteResponse::Error(format!("Failed to start service \"{name}\": {err}"))
                }
            }
        }
        RemoteCommand::Stop(name) => match registry.service_stop(name.clone()).await {
            Ok(_) => RemoteResponse::Success(format!("Service \"{name}\" stop initiated.")),
            Err(err) => RemoteResponse::Error(format!("Failed to stop service \"{name}\": {err}")),
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
        RemoteCommand::Status(name) => match registry
            .with_service(&name, |service| Ok(service.status()))
            .await
        {
            Ok(status) => RemoteResponse::Status(status),
            Err(err) => RemoteResponse::Error(err.to_string()),
        },
        RemoteCommand::List => match registry.service_list_all().await {
            Ok(list) => RemoteResponse::List(list),
            Err(err) => RemoteResponse::Error(format!("Failed to retrieve service list: {err}")),
        },
        RemoteCommand::Shutdown => {
            info!("Shutdown RemoteCommand received.");
            // Don't await shutdown here, just trigger it and respond
            tokio::spawn(shutdown(registry, false));
            RemoteResponse::ShuttingDown // Respond immediately
        }
    }
}

async fn shutdown(registry: ServiceRegistry, triggered_by_signal: bool) -> Result<(), Error> {
    info!("Initiating daemon shutdown...");
    registry.shutdown().await?;

    close_socket().await;

    info!("Goodbye");

    if triggered_by_signal {
        process::exit(0);
    }

    Ok(())
}
