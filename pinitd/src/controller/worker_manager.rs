use std::{collections::HashMap, sync::Arc, time::Duration};

use pinitd_common::{UID, WORKER_CONTROLLER_POLL_INTERVAL, WORKER_SOCKET_ADDRESS, WorkerIdentity};
use tokio::{
    net::TcpListener,
    sync::{Mutex, mpsc, oneshot},
    time::{sleep, timeout},
};

use crate::{
    error::{Error, Result},
    worker::{
        connection::{Connection, WorkerConnection},
        process::WorkerProcess,
        protocol::WorkerEvent,
    },
};

/// Manages multiple workers across different UIDs and SE info combinations
pub struct WorkerManager {
    /// Active worker connections by UID + se_info
    workers: Arc<Mutex<HashMap<WorkerIdentity, WorkerConnection>>>,
    /// Pending worker connection requests (WorkerIdentity -> completion channel)
    pending_connections:
        Arc<Mutex<HashMap<WorkerIdentity, Vec<oneshot::Sender<WorkerConnection>>>>>,
    /// Global event channel for all worker events
    event_tx: mpsc::Sender<WorkerEvent>,
    /// Allow the controller to completely disable worker spawns in a post-exploit environment
    disable_spawns: Arc<Mutex<bool>>,
}

impl WorkerManager {
    pub fn new(event_tx: mpsc::Sender<WorkerEvent>) -> Self {
        Self {
            workers: Arc::new(Mutex::new(HashMap::new())),
            pending_connections: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
            disable_spawns: Arc::new(Mutex::new(false)),
        }
    }

    /// Start the global worker listener that handles all worker connections
    pub async fn start_listener(&self) -> Result<()> {
        let socket = TcpListener::bind(WORKER_SOCKET_ADDRESS).await?;
        info!("Worker manager listening on {}", WORKER_SOCKET_ADDRESS);

        let workers = self.workers.clone();
        let pending_connections = self.pending_connections.clone();
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            loop {
                match socket.accept().await {
                    Ok((stream, addr)) => {
                        info!("New worker connection from {}", addr);
                        let workers = workers.clone();
                        let pending_connections = pending_connections.clone();
                        let event_tx = event_tx.clone();

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_new_connection(
                                stream,
                                workers,
                                pending_connections,
                                event_tx,
                            )
                            .await
                            {
                                error!("Failed to handle worker connection: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept worker connection: {}", e);
                        sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });

        Ok(())
    }

    async fn handle_new_connection(
        stream: tokio::net::TcpStream,
        workers: Arc<Mutex<HashMap<WorkerIdentity, WorkerConnection>>>,
        pending_connections: Arc<
            Mutex<HashMap<WorkerIdentity, Vec<oneshot::Sender<WorkerConnection>>>>,
        >,
        event_tx: mpsc::Sender<WorkerEvent>,
    ) -> Result<()> {
        // Create connection from stream
        let connection = Connection::from(stream, true);

        let worker_connection = timeout(
            Duration::from_secs(10),
            WorkerConnection::from_connection(connection, event_tx.clone()),
        )
        .await??;

        let worker_pid = worker_connection.pid();
        let worker_identity = worker_connection.identity();

        info!(
            "Worker for {worker_identity:?}, PID {worker_pid} successfully connected and identified",
        );

        // Register worker
        {
            let mut workers_lock = workers.lock().await;
            workers_lock.insert(worker_identity.clone(), worker_connection.clone());
            info!("Added worker for {worker_identity:?} to active connections");
        }

        // Notify any pending requests for this worker
        {
            let mut pending_lock = pending_connections.lock().await;
            if let Some(senders) = pending_lock.remove(&worker_identity) {
                info!(
                    "Fulfilling {} pending requests for worker {worker_identity:?}",
                    senders.len(),
                );
                for sender in senders {
                    let _ = sender.send(worker_connection.clone());
                }
            } else {
                info!("No pending requests found for worker {worker_identity:?}");
            }
        }

        // Start monitoring this worker connection
        let workers_monitor = workers.clone();
        let worker_identity_monitor = worker_identity.clone();
        tokio::spawn(async move {
            worker_connection.monitor_until_disconnect().await;
            info!(
                "Worker {worker_identity_monitor:?} disconnected, removing from active connections",
            );
            workers_monitor
                .lock()
                .await
                .remove(&worker_identity_monitor);
        });

        Ok(())
    }

    pub async fn get_worker_spawning_if_necessary(
        &self,
        uid: UID,
        se_info: Option<String>,
        launch_package: Option<String>,
    ) -> Result<WorkerConnection> {
        let identity = WorkerIdentity::new(uid, se_info);
        match self.get_worker_for_identity(&identity).await {
            Ok(connection) => Ok(connection),
            Err(_) => self.spawn_worker(&identity, launch_package).await,
        }
    }

    pub async fn get_worker_for_identity(
        &self,
        identity: &WorkerIdentity,
    ) -> Result<WorkerConnection> {
        let workers_lock = self.workers.lock().await;
        if let Some(connection) = workers_lock.get(identity) {
            if connection.is_healthy().await {
                return Ok(connection.clone());
            }
        }

        Err(Error::WorkerProtocolError(
            "No active connection to worker".into(),
        ))
    }

    pub async fn all_workers(&self) -> Vec<WorkerConnection> {
        self.workers
            .lock()
            .await
            .values()
            .map(|worker| worker.clone())
            .collect()
    }

    async fn spawn_worker(
        &self,
        identity: &WorkerIdentity,
        launch_package: Option<String>,
    ) -> Result<WorkerConnection> {
        if *self.disable_spawns.lock().await {
            return Err(Error::WorkerProtocolError(
                "Worker spawns are disabled".into(),
            ));
        }

        // No existing worker, spawn new one
        info!("Spawning new worker for {identity:?}");
        let (tx, rx) = oneshot::channel();

        // Mark connection pending
        {
            let mut pending_lock = self.pending_connections.lock().await;
            pending_lock
                .entry(identity.clone())
                .or_insert_with(Vec::new)
                .push(tx);
        }

        WorkerProcess::spawn(identity, launch_package).await?;

        match timeout(Duration::from_secs(15), rx).await {
            Ok(Ok(connection)) => {
                info!("Successfully connected to worker for {identity:?}");
                Ok(connection)
            }
            Ok(Err(_)) => {
                error!("Worker connection channel closed for {identity:?}");
                Err(crate::error::Error::Unknown(format!(
                    "Worker connection channel closed for {identity:?}",
                )))
            }
            Err(_) => {
                // Timeout occurred, but check if worker is available anyway
                info!("Timeout on channel for {identity:?}, checking if worker is available",);

                {
                    let workers_lock = self.workers.lock().await;
                    if let Some(connection) = workers_lock.get(&identity) {
                        if connection.is_healthy().await {
                            info!("Worker for {identity:?} is now available after timeout");
                            return Ok(connection.clone());
                        } else {
                            info!("Worker for {identity:?} found but not healthy");
                        }
                    } else {
                        info!("No worker found for {identity:?} after timeout");
                    }
                }

                error!("Timeout waiting for worker {:?} to connect", identity);
                // Clean up pending request
                let mut pending_lock = self.pending_connections.lock().await;
                if let Some(senders) = pending_lock.get_mut(&identity) {
                    info!(
                        "Cleaning up {} pending senders for {identity:?}",
                        senders.len(),
                    );
                    // Remove all pending senders
                    senders.retain(|_| false);
                } else {
                    info!("No pending senders to clean up for {identity:?}");
                }
                Err(crate::error::Error::Unknown(format!(
                    "Timeout waiting for worker {:?} to connect",
                    identity
                )))
            }
        }
    }

    pub async fn disable_spawning(&self) {
        *self.disable_spawns.lock().await = true;
    }

    pub async fn wait_for_worker_reconnections(&self) -> Result<()> {
        let wait_timeout = WORKER_CONTROLLER_POLL_INTERVAL * 4;

        info!(
            "Waiting {}ms for existing workers to reconnect...",
            wait_timeout.as_millis()
        );

        sleep(wait_timeout).await;

        info!("Worker reconnection wait period ended");
        Ok(())
    }

    pub async fn shutdown_all(&self) -> Result<()> {
        let workers_lock = self.workers.lock().await;
        for (identity, connection) in workers_lock.iter() {
            info!("Shutting down worker for {:?}", identity);
            let mut connection = connection.clone();
            connection.shutdown().await;
        }
        Ok(())
    }
}
