use std::time::Duration;

use pinitd_common::WORKER_SOCKET_ADDRESS;
use tokio::{
    net::TcpListener,
    sync::{broadcast, mpsc},
    time::sleep,
};

use crate::{
    error::Result,
    registry::controller::ControllerRegistry,
    types::BaseService,
    worker::{
        connection::{WorkerConnection, WorkerConnectionStatus},
        process::WorkerProcess,
    },
};

pub(crate) struct StartWorkerState {
    pub connection: WorkerConnection,
    pub worker_service_update_rx: mpsc::Receiver<BaseService>,
    pub worker_connected_rx: broadcast::Receiver<WorkerConnectionStatus>,
}

impl StartWorkerState {
    pub async fn start() -> Result<Self> {
        let socket = TcpListener::bind(&WORKER_SOCKET_ADDRESS).await?;
        info!("Listening for worker");

        let (worker_connected_tx, mut worker_connected_rx) =
            broadcast::channel::<WorkerConnectionStatus>(10);

        // TODO: Retry handling
        info!("Spawning worker");
        WorkerProcess::spawn_with_retries(5, worker_connected_rx.resubscribe())?;
        info!("Spawning worker sent");

        let (worker_service_update_tx, worker_service_update_rx) = mpsc::channel::<BaseService>(10);

        tokio::spawn(async move {
            let mut socket = socket;

            loop {
                info!("Attempting to connect to worker");
                let mut connection =
                    match WorkerConnection::open(&mut socket, worker_service_update_tx.clone())
                        .await
                    {
                        Ok(connection) => connection,
                        Err(err) => {
                            error!("Error connecting to worker: {err}");
                            sleep(Duration::from_secs(2)).await;
                            continue;
                        }
                    };

                info!("Worker connection established");
                let _ =
                    worker_connected_tx.send(WorkerConnectionStatus::Connected(connection.clone()));

                // Make sure connection stays available
                connection.subscribe_for_disconnect().await;
                info!("Worker disconnected");
                let _ = worker_connected_tx.send(WorkerConnectionStatus::Disconnected);
                // Loop again and attempt to reconnect
            }
        });

        info!("Waiting for worker to report back");
        loop {
            match WorkerConnectionStatus::await_update(&mut worker_connected_rx).await {
                WorkerConnectionStatus::Connected(connection) => {
                    info!("Worker spawn message received. Continuing...");
                    return Ok(Self {
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
}

pub fn start_worker_update_watcher(
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
