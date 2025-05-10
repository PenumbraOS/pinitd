use std::{process, time::Duration};

use pinitd_common::{
    CONTROL_SOCKET_ADDRESS, WORKER_SOCKET_ADDRESS,
    bincode::Bincodable,
    create_core_directories,
    protocol::{CLICommand, CLIResponse},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    signal::unix::{SignalKind, signal},
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::sleep,
};
use tokio_util::sync::CancellationToken;

use crate::{
    error::Result,
    registry::{Registry, controller::ControllerRegistry},
    types::BaseService,
    worker::{connection::WorkerConnection, process::WorkerProcess},
};

#[derive(Clone)]
pub struct Controller {
    registry: ControllerRegistry,
}

impl Controller {
    pub async fn specialize() -> Result<()> {
        create_core_directories();

        let (worker_connection, worker_service_update_rx) = start_worker().await?;

        let registry = ControllerRegistry::new(worker_connection).await?;
        start_worker_update_watcher(registry.clone(), worker_service_update_rx);
        let controller = Controller { registry };

        controller.registry.load_from_disk().await?;
        controller.registry.autostart_all().await?;

        let shutdown_token = CancellationToken::new();
        let shutdown_signal = setup_signal_watchers(shutdown_token.clone())?;

        info!("Controller started");
        let controller_clone = controller.clone();
        tokio::spawn(async move {
            let _ = controller_clone
                .start_cli_listener(shutdown_token.clone())
                .await;
        });

        let _ = shutdown_signal.await;
        info!("Shutting down");

        shutdown(controller.registry).await?;

        info!("After shutdown");

        Ok(())
    }

    async fn start_cli_listener(&self, shutdown_token: CancellationToken) -> Result<()> {
        let control_socket = TcpListener::bind(&CONTROL_SOCKET_ADDRESS).await?;
        info!("Listening for CLI");

        loop {
            match control_socket.accept().await {
                Ok((mut stream, _)) => {
                    info!("Accepted new client connection");
                    let controller_clone = self.clone();
                    let shutdown_token_clone = shutdown_token.clone();
                    tokio::spawn(async move {
                        match controller_clone
                            .handle_command(&mut stream, shutdown_token_clone)
                            .await
                        {
                            Ok(response) => match response.encode() {
                                Ok(data) => {
                                    let _ = stream.write_all(&data).await;
                                    let _ = stream.shutdown().await;
                                }
                                Err(err) => error!("Error responding to client: {err:?}"),
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
    }

    async fn handle_command<T>(
        &self,
        stream: &mut T,
        shutdown_token: CancellationToken,
    ) -> Result<CLIResponse>
    where
        T: AsyncRead + Unpin,
    {
        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer).await?;

        let (command, _) = CLICommand::decode(&buffer)?;
        info!("Received CLICommand: {:?}", command);

        let response = self
            .registry
            .process_remote_command(command, shutdown_token)
            .await;

        Ok(response)
    }
}

async fn start_worker() -> Result<(WorkerConnection, mpsc::Receiver<BaseService>)> {
    let socket = TcpListener::bind(&WORKER_SOCKET_ADDRESS).await?;
    info!("Listening for worker");

    // TODO: Retry handling
    info!("Spawning worker");
    WorkerProcess::spawn().await?;
    info!("Spawning worker sent");

    let (worker_service_update_tx, worker_service_update_rx) = mpsc::channel::<BaseService>(10);
    let (worker_connected_tx, worker_connected_rx) = oneshot::channel::<WorkerConnection>();

    tokio::spawn(async move {
        let mut socket = socket;

        let mut worker_connected_tx = Some(worker_connected_tx);

        loop {
            info!("Attempting to connect to worker");
            let mut connection =
                match WorkerConnection::open(&mut socket, worker_service_update_tx.clone()).await {
                    Ok(connection) => connection,
                    Err(err) => {
                        error!("Error connecting to worker: {err}");
                        sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                };

            info!("Worker connection established");
            if let Some(tx) = worker_connected_tx {
                let _ = tx.send(connection.clone());
                worker_connected_tx = None;
            }

            // Make sure connection stays available
            connection.subscribe_for_disconnect().await;
            // Loop again and attempt to reconnect
        }
    });

    info!("Waiting for worker to report back");
    let connection = worker_connected_rx.await?;
    info!("Worker spawn message received. Continuing...");

    Ok((connection, worker_service_update_rx))
}

fn start_worker_update_watcher(
    registry: ControllerRegistry,
    mut worker_service_update_rx: mpsc::Receiver<BaseService>,
) {
    tokio::spawn(async move {
        loop {
            let service = worker_service_update_rx
                .recv()
                .await
                .expect("Channel unexpectedly closed");

            let name = service.config.name.clone();

            if let Err(err) = registry.clone().worker_service_update(service).await {
                error!("Could not update remote service \"{name}\": {err}")
            }
        }
    });
}

fn setup_signal_watchers(shutdown_token: CancellationToken) -> Result<JoinHandle<()>> {
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

async fn shutdown(registry: ControllerRegistry) -> Result<()> {
    info!("Initiating daemon shutdown...");
    registry.shutdown().await?;

    info!("Goodbye");

    process::exit(0);
}
