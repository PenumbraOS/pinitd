use std::collections::HashMap;

use tokio::select;
use tokio_util::sync::CancellationToken;

use crate::{
    error::Result,
    registry::{Registry, local::LocalRegistry},
    worker::{
        connection::ControllerConnection,
        protocol::{WorkerCommand, WorkerResponse},
    },
};

pub struct WorkerProcess;

impl WorkerProcess {
    pub async fn specialize() -> Result<()> {
        info!("Connecting to controller");

        let mut connection = ControllerConnection::open().await?;
        info!("Controller connected");

        let mut registry = LocalRegistry::worker(connection.clone())?;
        let token = CancellationToken::new();

        loop {
            select! {
                _ = token.cancelled() => {

                }
                result = connection.read_command() => match result {
                    Ok(command) => {
                        info!("Received command {command:?}");

                        let response = match handle_command(command, &mut registry, &token).await {
                            Ok(response) => response,
                            Err(err) => {
                                let err = format!("Error processing command: {err}");
                                error!("{err}");
                                WorkerResponse::Error(err)
                            }
                        };

                        connection.write_response(response).await?;
                    }
                    Err(err) => {
                        error!("Error processing command packet: {err}");
                        info!("Reconnecting to controller");
                        connection = ControllerConnection::open().await?;
                    }
                }
            }
        }
    }

    /// Spawn a remote process to act as the system worker
    #[cfg(target_os = "android")]
    pub async fn spawn() -> Result<()> {
        unimplemented!()
    }

    /// Spawn a remote process to act as the system worker
    #[cfg(not(target_os = "android"))]
    pub async fn spawn() -> Result<()> {
        let process_path = std::env::args().next().unwrap();
        tokio::process::Command::new(process_path)
            .arg("worker")
            .spawn()?;
        Ok(())
    }
}

async fn handle_command(
    command: WorkerCommand,
    registry: &mut LocalRegistry,
    token: &CancellationToken,
) -> Result<WorkerResponse> {
    match command {
        WorkerCommand::Create(service_config) => {
            // Register config
            registry.insert_unit(service_config, true).await?;
        }
        WorkerCommand::Destroy(name) => {
            // Delete config
            registry.remove_unit(name).await?;
        }
        WorkerCommand::Start(name) => {
            registry.service_start(name).await?;
        }
        WorkerCommand::Stop(name) => {
            registry.service_stop(name).await?;
        }
        WorkerCommand::Restart(name) => {
            registry.service_restart(name).await?;
        }
        WorkerCommand::Status => {
            let status = registry.service_list_all().await?;
            let status_iter = status.into_iter().map(|s| (s.name, s.state));
            return Ok(WorkerResponse::Status(HashMap::from_iter(status_iter)));
        }
        WorkerCommand::Shutdown => {
            let _ = registry.shutdown().await;
            // Trigger process shutdown
            token.cancel();
            return Ok(WorkerResponse::ShuttingDown);
        }
    };

    Ok(WorkerResponse::Success)
}
