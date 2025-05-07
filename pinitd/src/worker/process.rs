use std::collections::HashMap;

use pinitd_common::{CONTROL_SOCKET_ADDRESS, bincode::Bincodable};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    select,
};
use tokio_util::sync::CancellationToken;

use crate::{
    error::Error,
    registry::ServiceRegistry,
    worker::protocol::{WorkerCommand, WorkerResponse},
};

pub struct WorkerProcess;

impl WorkerProcess {
    pub async fn create() -> Result<(), Error> {
        info!("Connecting to controller");
        let mut stream = TcpStream::connect(CONTROL_SOCKET_ADDRESS).await?;
        info!("Controller connected");

        let mut registry = ServiceRegistry::empty()?;
        let token = CancellationToken::new();

        loop {
            let mut len_bytes = [0; std::mem::size_of::<u64>()];

            select! {
                _ = token.cancelled() => {

                }
                result = stream.read_exact(&mut len_bytes) => match result {
                    Ok(_) => {
                        stream.read_exact(&mut len_bytes).await?;
                        let len = u64::from_le_bytes(len_bytes);

                        let mut buffer = vec![0; len as usize];
                        stream.read_exact(&mut buffer).await?;

                        let (command, _) = WorkerCommand::decode(&buffer)?;
                        info!("Received command {command:?}");

                        let response = match handle_command(command, &mut registry, &token).await {
                            Ok(response) => response,
                            Err(err) => {
                                let err = format!("Error processing command: {err}");
                                error!("{err}");
                                WorkerResponse::Error(err)
                            }
                        };

                        match response.encode() {
                            Ok(data) => {
                                let _ = stream.write_all(&data).await;
                                let _ = stream.shutdown().await;
                            }
                            Err(err) => error!("Error responding to controller: {err:?}"),
                        }
                    },
                    Err(err) => error!("Error reading from controller: {err:?}"),
                }
            }
        }
    }
}

async fn handle_command(
    command: WorkerCommand,
    registry: &mut ServiceRegistry,
    token: &CancellationToken,
) -> Result<WorkerResponse, Error> {
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
            registry.shutdown();
            // Trigger process shutdown
            token.cancel();
            return Ok(WorkerResponse::ShuttingDown);
        }
    };

    Ok(WorkerResponse::Success)
}
