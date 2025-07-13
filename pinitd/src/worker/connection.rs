use std::{error::Error, sync::Arc, time::Duration};

use pinitd_common::{
    WORKER_SOCKET_ADDRESS,
    protocol::writable::{ProtocolRead, ProtocolWrite},
};
use tokio::{
    io,
    net::{
        TcpListener, TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    sync::{
        Mutex, MutexGuard, broadcast, mpsc,
        watch::{self, Receiver},
    },
    task::JoinHandle,
    time::{sleep, timeout},
};

use crate::{error::Result, types::BaseService};

use super::protocol::{WorkerCommand, WorkerResponse};

#[derive(Clone)]
pub enum WorkerConnectionStatus {
    Connected(WorkerConnection),
    Disconnected,
}

impl WorkerConnectionStatus {
    pub async fn await_update(
        worker_connected_rx: &mut broadcast::Receiver<WorkerConnectionStatus>,
    ) -> WorkerConnectionStatus {
        worker_connected_rx
            .recv()
            .await
            .unwrap_or_else(|_| WorkerConnectionStatus::Disconnected)
    }
}

/// Connection held by Controller to transfer data to/from Worker
#[derive(Clone)]
pub struct WorkerConnection {
    connection: Connection,
    read: Arc<Mutex<mpsc::Receiver<WorkerResponse>>>,
    _read_loop: Arc<Mutex<JoinHandle<()>>>,
    // When set, ignore socket errors as we're shutting down
    in_shutdown: bool,
}

/// Connection held by Worker to transfer data to/from Controller
#[derive(Clone)]
pub enum ControllerConnection {
    WithConnection(Connection),
    Disabled,
}

#[derive(Clone)]
pub struct Connection {
    read: Arc<Mutex<OwnedReadHalf>>,
    write: Arc<Mutex<OwnedWriteHalf>>,
    health_tx: watch::Sender<bool>,
    health_rx: Receiver<bool>,
    is_controller: bool,
}

impl Connection {
    fn from(stream: TcpStream, is_controller: bool) -> Self {
        let (read, write) = stream.into_split();

        let (health_tx, health_rx) = watch::channel(true);

        Connection {
            read: Arc::new(Mutex::new(read)),
            write: Arc::new(Mutex::new(write)),
            health_tx,
            health_rx,
            is_controller,
        }
    }

    fn is_connected(&self) -> bool {
        *self.health_rx.borrow()
    }

    async fn subscribe_for_disconnect(&mut self) {
        let _ = self.health_rx.wait_for(|value| !*value).await;
    }

    fn mark_disconnected(&self, message: String) {
        error!(
            "Controller/worker ({}) connection lost. Error: {message}",
            self.is_controller
        );
        let _ = self.health_tx.send(false);
    }
}

impl WorkerConnection {
    pub async fn open(
        socket: &mut TcpListener,
        worker_service_update_tx: mpsc::Sender<BaseService>,
    ) -> Result<Self> {
        let (stream, _) = socket.accept().await?;
        info!("Connected to worker");

        let connection = Connection::from(stream, true);

        let (read_tx, read_rx) = mpsc::channel::<WorkerResponse>(10);
        let read_loop = WorkerConnection::start_read_loop(
            connection.clone(),
            read_tx,
            worker_service_update_tx,
        )
        .await;

        Ok(WorkerConnection {
            connection,
            read: Arc::new(Mutex::new(read_rx)),
            _read_loop: Arc::new(Mutex::new(read_loop)),
            in_shutdown: false,
        })
    }

    async fn start_read_loop(
        connection: Connection,
        read_tx: mpsc::Sender<WorkerResponse>,
        worker_service_update_tx: mpsc::Sender<BaseService>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            // Permanently hold read lock
            let mut read_lock = connection.read.lock().await;

            loop {
                match WorkerResponse::read(&mut *read_lock).await {
                    Ok(response) => {
                        match response {
                            WorkerResponse::ServiceUpdate(service) => {
                                // We probably don't care about these errors
                                let _ = worker_service_update_tx.send(service).await;
                            }
                            _ => {
                                // We probably don't care about these errors
                                let _ = read_tx.send(response).await;
                            }
                        }
                    }
                    Err(err) => {
                        if !connection.is_connected() {
                            return;
                        }
                        if let Some(source) = err.source() {
                            if let Some(err) = source.downcast_ref::<io::Error>() {
                                if err.kind() == io::ErrorKind::UnexpectedEof {
                                    // Connection lost, abort
                                    return;
                                }
                            }
                        }
                        error!("Failed to read from worker: {err}");
                        sleep(Duration::from_millis(1000)).await;
                    }
                }
            }
        })
    }

    pub async fn write_command(&self, command: WorkerCommand) -> Result<WorkerResponse> {
        match timeout(Duration::from_millis(200), async move {
            info!("Sending worker command");
            let mut write = self.connection.write.lock().await;
            command.write(&mut *write).await?;

            if self.in_shutdown {
                return Ok(WorkerResponse::ShuttingDown);
            }

            match self.read.lock().await.recv().await {
                Some(response) => Ok(response),
                None => Err(crate::error::Error::WorkerProtocolError(
                    "Connection closed".into(),
                )),
            }
        })
        .await
        {
            Ok(Ok(response)) => {
                if let WorkerResponse::Error(err) = response {
                    // Convert into local error
                    Err(crate::error::Error::WorkerProtocolError(err))
                } else {
                    Ok(response)
                }
            }
            Ok(Err(err)) => {
                // Any error immediately closes the connection
                self.connection.mark_disconnected(err.to_string());
                Err(err)
            }
            Err(err) => {
                self.connection.mark_disconnected(err.to_string());
                Err(err.into())
            }
        }
    }

    #[allow(dead_code)]
    pub fn is_connected(&self) -> bool {
        self.connection.is_connected()
    }

    pub async fn subscribe_for_disconnect(&mut self) {
        self.connection.subscribe_for_disconnect().await;
    }

    pub async fn shutdown(&mut self) {
        self.in_shutdown = true;
        self._read_loop.lock().await.abort();
    }
}

impl ControllerConnection {
    pub async fn open() -> Result<Self> {
        let stream = TcpStream::connect(WORKER_SOCKET_ADDRESS).await?;
        info!("Connected to controller");

        Ok(ControllerConnection::WithConnection(Connection::from(
            stream, false,
        )))
    }

    pub async fn read_command(&self) -> Result<WorkerCommand> {
        match self {
            ControllerConnection::WithConnection(connection) => {
                let mut read = connection.read.lock().await;
                info!("Awaiting command");
                match WorkerCommand::read(&mut *read).await {
                    Ok(command) => Ok(command),
                    Err(err) => {
                        // Any error immediately closes the connection
                        connection.mark_disconnected(err.to_string());
                        Err(crate::error::Error::CommonError(err))
                    }
                }
            }
            ControllerConnection::Disabled => Err(crate::error::Error::Unknown(
                "Cannot read controller from controller".into(),
            )),
        }
    }

    pub async fn write_response(&self, response: WorkerResponse) -> Result<()> {
        let write_lock = self.acquire_write_lock().await?;
        self.write_response_with_lock(write_lock, response).await
    }

    pub async fn acquire_write_lock(&self) -> Result<MutexGuard<'_, OwnedWriteHalf>> {
        match self {
            ControllerConnection::WithConnection(connection) => Ok(connection.write.lock().await),
            ControllerConnection::Disabled => Err(crate::error::Error::Unknown(
                "Cannot acquire controller write lock from controller".into(),
            )),
        }
    }

    pub async fn write_response_with_lock(
        &self,
        mut write_lock: MutexGuard<'_, OwnedWriteHalf>,
        response: WorkerResponse,
    ) -> Result<()> {
        match response.write(&mut *write_lock).await {
            Ok(_) => Ok(()),
            Err(err) => {
                // Any error immediately closes the connection
                if let ControllerConnection::WithConnection(connection) = self {
                    connection.mark_disconnected(err.to_string());
                }
                Err(crate::error::Error::CommonError(err))
            }
        }
    }
}
