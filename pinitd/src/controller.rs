use std::{process, time::Duration};

use android_31317_exploit::force_clear_exploit;
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
    sync::mpsc::{self, Receiver},
    task::JoinHandle,
    time::sleep,
};
use tokio_util::sync::CancellationToken;

use crate::{
    error::Result,
    registry::{Registry, controller::ControllerRegistry},
    types::BaseService,
    worker::{
        connection::{WorkerConnection, WorkerConnectionStatus},
        process::WorkerProcess,
    },
};

#[derive(Clone)]
pub struct Controller {
    registry: ControllerRegistry,
}

impl Controller {
    pub async fn specialize() -> Result<()> {
        create_core_directories();

        let _ = force_clear_exploit();
        info!("Delaying to allow Zygote to settle");
        sleep(Duration::from_millis(500)).await;

        let StartWorkerState {
            connection,
            worker_service_update_rx,
            worker_connected_rx,
        } = start_worker().await?;

        let registry = ControllerRegistry::new(connection).await?;
        let controller = Controller { registry };

        controller.registry.load_from_disk().await?;

        let shutdown_token = CancellationToken::new();
        let shutdown_signal = setup_signal_watchers(shutdown_token.clone())?;

        start_worker_update_watcher(controller.registry.clone(), worker_service_update_rx);

        info!("Controller started");
        let controller_clone = controller.clone();
        tokio::spawn(async move {
            let _ = controller_clone
                .start_cli_listener(shutdown_token.clone())
                .await;
        });

        controller.start_worker_connection_listener(worker_connected_rx);

        info!("Autostarting services");
        controller.registry.autostart_all().await?;

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

    fn start_worker_connection_listener(
        &self,
        mut worker_connected_rx: Receiver<WorkerConnectionStatus>,
    ) {
        let inner_controller = self.clone();
        tokio::spawn(async move {
            loop {
                let update = await_connection_status_update(&mut worker_connected_rx).await;
                inner_controller
                    .clone()
                    .registry
                    .update_worker_connection(update)
                    .await;
            }
        });
    }
}

struct StartWorkerState {
    connection: WorkerConnection,
    worker_service_update_rx: Receiver<BaseService>,
    worker_connected_rx: Receiver<WorkerConnectionStatus>,
}

async fn start_worker() -> Result<StartWorkerState> {
    let socket = TcpListener::bind(&WORKER_SOCKET_ADDRESS).await?;
    info!("Listening for worker");

    // TODO: Retry handling
    info!("Spawning worker");
    WorkerProcess::spawn().await?;
    info!("Spawning worker sent");

    let (worker_service_update_tx, worker_service_update_rx) = mpsc::channel::<BaseService>(10);
    let (worker_connected_tx, mut worker_connected_rx) =
        mpsc::channel::<WorkerConnectionStatus>(10);

    tokio::spawn(async move {
        let mut socket = socket;

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
            let _ = worker_connected_tx
                .send(WorkerConnectionStatus::Connected(connection.clone()))
                .await;

            // Make sure connection stays available
            connection.subscribe_for_disconnect().await;
            let _ = worker_connected_tx
                .send(WorkerConnectionStatus::Disconnected)
                .await;
            // Loop again and attempt to reconnect
        }
    });

    info!("Waiting for worker to report back");
    loop {
        match await_connection_status_update(&mut worker_connected_rx).await {
            WorkerConnectionStatus::Connected(connection) => {
                info!("Worker spawn message received. Continuing...");
                return Ok(StartWorkerState {
                    connection,
                    worker_connected_rx,
                    worker_service_update_rx,
                });
            }
            WorkerConnectionStatus::Disconnected => {
                warn!(
                    "Worker unexpectedly disconnected before first connection. Awaiting connection..."
                );
            }
        }
    }
}

async fn await_connection_status_update(
    worker_connected_rx: &mut Receiver<WorkerConnectionStatus>,
) -> WorkerConnectionStatus {
    worker_connected_rx
        .recv()
        .await
        .unwrap_or_else(|| WorkerConnectionStatus::Disconnected)
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

            if let Err(err) = registry.clone().update_worker_service(service).await {
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
