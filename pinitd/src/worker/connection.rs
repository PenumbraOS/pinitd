use std::{sync::Arc, time::Duration};

use pinitd_common::WORKER_SOCKET_ADDRESS;
use tokio::{
    net::{
        TcpListener, TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    sync::{
        Mutex,
        watch::{self, Receiver, Sender},
    },
    time::timeout,
};

use crate::error::{Error, Result};

use super::protocol::{WorkerCommand, WorkerRead, WorkerResponse, WorkerWrite};

/// Connection held by Controller to transfer data to/from Worker
#[derive(Clone)]
pub struct WorkerConnection {
    connection: Connection,
}

/// Connection held by Worker to transfer data to/from Controller
pub struct ControllerConnection {
    connection: Connection,
}

#[derive(Clone)]
struct Connection {
    read: Arc<Mutex<OwnedReadHalf>>,
    write: Arc<Mutex<OwnedWriteHalf>>,
    health_tx: Sender<bool>,
    health_rx: Receiver<bool>,
}

impl Connection {
    fn from(stream: TcpStream) -> Self {
        let (read, write) = stream.into_split();

        let (health_tx, health_rx) = watch::channel(true);

        Connection {
            read: Arc::new(Mutex::new(read)),
            write: Arc::new(Mutex::new(write)),
            health_tx,
            health_rx,
        }
    }

    fn is_connected(&self) -> bool {
        *self.health_rx.borrow()
    }

    async fn subscribe_for_disconnect(&mut self) {
        let _ = self.health_rx.wait_for(|value| !*value).await;
    }

    fn mark_disconnected(&self) {
        let _ = self.health_tx.send(false);
    }
}

impl WorkerConnection {
    pub async fn open(socket: &mut TcpListener) -> Result<Self> {
        let (stream, _) = socket.accept().await?;
        info!("Connected to worker");

        Ok(WorkerConnection {
            connection: Connection::from(stream),
        })
    }

    pub async fn write_command(&self, command: WorkerCommand) -> Result<WorkerResponse> {
        match timeout(Duration::from_millis(200), async move {
            let mut write = self.connection.write.lock().await;
            command
                .write(&mut *write)
                .await
                .map(|_| async {
                    let mut read = self.connection.read.lock().await;
                    WorkerResponse::read(&mut *read).await
                })?
                .await
        })
        .await
        {
            Ok(Ok(response)) => {
                if let WorkerResponse::Error(err) = response {
                    // Convert into local error
                    Err(Error::WorkerProtocolError(err))
                } else {
                    Ok(response)
                }
            }
            Ok(Err(err)) => {
                // Any error immediately closes the connection
                self.connection.mark_disconnected();
                Err(err)
            }
            Err(err) => {
                self.connection.mark_disconnected();
                Err(err.into())
            }
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connection.is_connected()
    }

    pub async fn subscribe_for_disconnect(&mut self) {
        self.connection.subscribe_for_disconnect().await;
    }
}

impl ControllerConnection {
    pub async fn open() -> Result<Self> {
        let stream = TcpStream::connect(WORKER_SOCKET_ADDRESS).await?;
        info!("Connected to controller");

        Ok(ControllerConnection {
            connection: Connection::from(stream),
        })
    }

    pub async fn read_command(&self) -> Result<WorkerCommand> {
        let mut read = self.connection.read.lock().await;
        match WorkerCommand::read(&mut *read).await {
            Ok(command) => Ok(command),
            Err(err) => {
                // Any error immediately closes the connection
                self.connection.mark_disconnected();
                Err(err)
            }
        }
    }

    pub async fn write_response(&self, response: WorkerResponse) -> Result<()> {
        let mut write = self.connection.write.lock().await;
        match response.write(&mut *write).await {
            Ok(_) => Ok(()),
            Err(err) => {
                // Any error immediately closes the connection
                self.connection.mark_disconnected();
                Err(err)
            }
        }
    }
}
