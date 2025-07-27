use std::{collections::HashMap, sync::Arc, time::Duration};

use pinitd_common::{UID, WORKER_SOCKET_ADDRESS};
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

/// Manages multiple workers across different UIDs
pub struct WorkerManager {
    /// Active worker connections by UID
    workers: Arc<Mutex<HashMap<UID, WorkerConnection>>>,
    /// Pending worker connection requests (UID -> completion channel)
    pending_connections: Arc<Mutex<HashMap<UID, Vec<oneshot::Sender<WorkerConnection>>>>>,
    /// Global event channel for all worker events
    event_tx: mpsc::Sender<WorkerEvent>,
}

impl WorkerManager {
    pub fn new(event_tx: mpsc::Sender<WorkerEvent>) -> Self {
        Self {
            workers: Arc::new(Mutex::new(HashMap::new())),
            pending_connections: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
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
        workers: Arc<Mutex<HashMap<UID, WorkerConnection>>>,
        pending_connections: Arc<Mutex<HashMap<UID, Vec<oneshot::Sender<WorkerConnection>>>>>,
        event_tx: mpsc::Sender<WorkerEvent>,
    ) -> Result<()> {
        // Create connection from stream
        let connection = Connection::from(stream, true);

        let worker_connection = timeout(
            Duration::from_secs(10),
            WorkerConnection::from_connection(connection, event_tx.clone()),
        )
        .await??;

        let worker_uid = worker_connection.uid();

        info!("Worker for UID {worker_uid:?} successfully connected and identified",);

        // Register worker
        {
            let mut workers_lock = workers.lock().await;
            workers_lock.insert(worker_uid.clone(), worker_connection.clone());
            info!("Added worker for UID {worker_uid:?} to active connections",);
        }

        // Notify any pending requests for this UID
        {
            let mut pending_lock = pending_connections.lock().await;
            if let Some(senders) = pending_lock.remove(&worker_uid) {
                info!(
                    "Fulfilling {} pending requests for UID {worker_uid:?}",
                    senders.len(),
                );
                for sender in senders {
                    let _ = sender.send(worker_connection.clone());
                }
            } else {
                info!("No pending requests found for UID {worker_uid:?}");
            }
        }

        // Start monitoring this worker connection
        let workers_monitor = workers.clone();
        let worker_uid_monitor = worker_uid.clone();
        tokio::spawn(async move {
            worker_connection.monitor_until_disconnect().await;
            info!("Worker {worker_uid_monitor:?} disconnected, removing from active connections",);
            workers_monitor.lock().await.remove(&worker_uid_monitor);
        });

        Ok(())
    }

    pub async fn get_worker_spawning_if_necessary(
        &self,
        uid: UID,
        se_info: Option<String>,
    ) -> Result<WorkerConnection> {
        match self.get_worker_for_uid(uid.clone()).await {
            Ok(connection) => Ok(connection),
            Err(_) => self.spawn_worker(uid, se_info).await,
        }
    }

    pub async fn get_worker_for_uid(&self, uid: UID) -> Result<WorkerConnection> {
        let workers_lock = self.workers.lock().await;
        if let Some(connection) = workers_lock.get(&uid) {
            if connection.is_healthy().await {
                return Ok(connection.clone());
            }
        }

        Err(Error::WorkerProtocolError(
            "No active connection to worker".into(),
        ))
    }

    async fn spawn_worker(&self, uid: UID, se_info: Option<String>) -> Result<WorkerConnection> {
        // No existing worker, spawn new one
        info!("Spawning new worker for UID {uid:?}");
        let (tx, rx) = oneshot::channel();

        // Mark connection pending
        {
            let mut pending_lock = self.pending_connections.lock().await;
            pending_lock
                .entry(uid.clone())
                .or_insert_with(Vec::new)
                .push(tx);
        }

        // Spawn the worker
        WorkerProcess::spawn(uid.clone(), se_info).await?;

        match timeout(Duration::from_secs(15), rx).await {
            Ok(Ok(connection)) => {
                info!("Successfully connected to worker for UID {uid:?}");
                Ok(connection)
            }
            Ok(Err(_)) => {
                error!("Worker connection channel closed for UID {uid:?}");
                Err(crate::error::Error::Unknown(format!(
                    "Worker connection channel closed for UID {uid:?}",
                )))
            }
            Err(_) => {
                // Timeout occurred, but check if worker is available anyway
                info!("Timeout on channel for UID {uid:?}, checking if worker is available",);

                {
                    let workers_lock = self.workers.lock().await;
                    if let Some(connection) = workers_lock.get(&uid) {
                        if connection.is_healthy().await {
                            info!("Worker for UID {uid:?} is now available after timeout");
                            return Ok(connection.clone());
                        } else {
                            info!("Worker for UID {uid:?} found but not healthy");
                        }
                    } else {
                        info!("No worker found for UID {uid:?} after timeout");
                    }
                }

                error!("Timeout waiting for worker {:?} to connect", uid);
                // Clean up pending request
                let mut pending_lock = self.pending_connections.lock().await;
                if let Some(senders) = pending_lock.get_mut(&uid) {
                    info!(
                        "Cleaning up {} pending senders for UID {uid:?}",
                        senders.len(),
                    );
                    // Remove all pending senders
                    senders.retain(|_| false);
                } else {
                    info!("No pending senders to clean up for UID {uid:?}");
                }
                Err(crate::error::Error::Unknown(format!(
                    "Timeout waiting for worker {:?} to connect",
                    uid
                )))
            }
        }
    }

    pub async fn shutdown_all(&self) -> Result<()> {
        let workers_lock = self.workers.lock().await;
        for (uid, connection) in workers_lock.iter() {
            info!("Shutting down worker for UID {:?}", uid);
            let mut connection = connection.clone();
            connection.shutdown().await;
        }
        Ok(())
    }
}
