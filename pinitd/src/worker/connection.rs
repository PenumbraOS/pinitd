use std::{error::Error, sync::Arc};

use pinitd_common::{
    UID, WORKER_SOCKET_ADDRESS,
    protocol::writable::{ProtocolRead, ProtocolWrite},
};
use tokio::{
    io,
    net::{
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    sync::{
        Mutex, MutexGuard, mpsc,
        watch::{self, Receiver},
    },
    task::JoinHandle,
    time::{Duration, sleep, timeout},
};

use crate::error::Result;

use super::protocol::{WorkerCommand, WorkerEvent, WorkerMessage, WorkerResponse, WorkerState};

/// Connection held by Controller to transfer data to/from Worker
#[derive(Clone)]
pub struct WorkerConnection {
    connection: Connection,
    uid: UID,
    se_info: String,
    pid: usize,
    read: Arc<Mutex<mpsc::Receiver<WorkerResponse>>>,
    _read_loop: Arc<Mutex<JoinHandle<()>>>,
    // When set, ignore socket errors as we're shutting down
    in_shutdown: bool,
}

/// Connection held by Worker to transfer data to/from Controller
#[derive(Clone)]
pub struct ControllerConnection(Connection);

#[derive(Clone)]
pub struct Connection {
    read: Arc<Mutex<OwnedReadHalf>>,
    write: Arc<Mutex<OwnedWriteHalf>>,
    health_tx: watch::Sender<bool>,
    health_rx: Receiver<bool>,
    is_controller: bool,
}

impl Connection {
    pub fn from(stream: TcpStream, is_controller: bool) -> Self {
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
    async fn start_read_loop(
        connection: Connection,
        read_tx: mpsc::Sender<WorkerResponse>,
        worker_event_tx: mpsc::Sender<WorkerEvent>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            // Permanently hold read lock
            let mut read_lock = connection.read.lock().await;

            loop {
                match WorkerMessage::read(&mut *read_lock).await {
                    Ok(message) => {
                        match message {
                            WorkerMessage::Response(response) => {
                                // Send command responses to the response channel
                                let _ = read_tx.send(response).await;
                            }
                            WorkerMessage::Event(event) => {
                                // Send events to the global event handler
                                let _ = worker_event_tx.send(event).await;
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

    pub fn uid(&self) -> &UID {
        &self.uid
    }

    pub fn se_info(&self) -> &String {
        &self.se_info
    }

    pub fn pid(&self) -> usize {
        self.pid
    }

    pub async fn is_healthy(&self) -> bool {
        self.connection.is_connected()
    }

    #[allow(dead_code)]
    pub fn is_connected(&self) -> bool {
        self.connection.is_connected()
    }

    pub async fn request_current_state(&self) -> Result<WorkerState> {
        let response = self
            .write_command(WorkerCommand::RequestCurrentState)
            .await?;

        match response {
            WorkerResponse::CurrentState(state) => Ok(state),
            _ => Err(crate::error::Error::WorkerProtocolError(
                "Unexpected response to RequestCurrentState".into(),
            )),
        }
    }

    pub async fn shutdown(&mut self) {
        self.in_shutdown = true;
        self._read_loop.lock().await.abort();
    }

    pub async fn from_connection(
        connection: Connection,
        worker_event_tx: mpsc::Sender<WorkerEvent>,
    ) -> Result<Self> {
        let (uid, pid, se_info) = {
            let mut read_lock = connection.read.lock().await;
            let message = WorkerMessage::read(&mut *read_lock).await?;

            match message {
                WorkerMessage::Event(WorkerEvent::WorkerRegistration {
                    worker_uid,
                    worker_pid,
                    worker_se_info,
                }) => {
                    info!("Worker identified as UID {worker_uid:?} with PID {worker_pid} and se_info {worker_se_info}",);
                    (worker_uid, worker_pid, worker_se_info)
                }
                _ => {
                    return Err(crate::error::Error::Unknown(
                        "First message from worker was not registration".into(),
                    ));
                }
            }
        };

        // Resend the registration event to the event handler
        let registration_event = WorkerEvent::WorkerRegistration {
            worker_uid: uid.clone(),
            worker_pid: pid,
            worker_se_info: se_info.clone(),
        };
        if let Err(e) = worker_event_tx.send(registration_event).await {
            error!("Failed to resend worker registration event: {}", e);
        }

        let (read_tx, read_rx) = mpsc::channel::<WorkerResponse>(10);
        let read_loop =
            WorkerConnection::start_read_loop(connection.clone(), read_tx, worker_event_tx).await;

        Ok(WorkerConnection {
            connection,
            uid,
            se_info,
            pid,
            read: Arc::new(Mutex::new(read_rx)),
            _read_loop: Arc::new(Mutex::new(read_loop)),
            in_shutdown: false,
        })
    }

    pub async fn monitor_until_disconnect(&self) {
        let mut connection = self.connection.clone();
        connection.subscribe_for_disconnect().await;
    }
}

impl ControllerConnection {
    pub async fn open() -> Result<Self> {
        let stream = timeout(Duration::from_secs(5), async move {
            TcpStream::connect(WORKER_SOCKET_ADDRESS).await
        })
        .await??;
        info!("Connected to controller");

        Ok(ControllerConnection(Connection::from(stream, false)))
    }

    pub async fn read_command(&self) -> Result<WorkerCommand> {
        let mut read = self.0.read.lock().await;
        match WorkerCommand::read(&mut *read).await {
            Ok(command) => Ok(command),
            Err(err) => {
                // Any error immediately closes the connection
                self.0.mark_disconnected(err.to_string());
                Err(crate::error::Error::CommonError(err))
            }
        }
    }

    pub async fn write_response(&self, message: WorkerMessage) -> Result<()> {
        let write_lock = self.acquire_write_lock().await?;
        self.write_message_with_lock(write_lock, message).await
    }

    pub async fn acquire_write_lock(&self) -> Result<MutexGuard<'_, OwnedWriteHalf>> {
        Ok(self.0.write.lock().await)
    }

    pub async fn write_message_with_lock(
        &self,
        mut write_lock: MutexGuard<'_, OwnedWriteHalf>,
        message: WorkerMessage,
    ) -> Result<()> {
        match message.write(&mut *write_lock).await {
            Ok(_) => Ok(()),
            Err(err) => {
                // Any error immediately closes the connection
                self.0.mark_disconnected(err.to_string());
                Err(crate::error::Error::CommonError(err))
            }
        }
    }
}
