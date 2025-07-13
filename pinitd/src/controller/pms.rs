use std::{collections::HashMap, sync::Arc};

use crate::{
    error::{Error, Result},
    registry::controller::ControllerRegistry,
};
use pinitd_common::{
    PMS_SOCKET_ADDRESS, ServiceRunState,
    bincode::Bincodable,
    protocol::{
        PMSFromRemoteCommand, PMSToRemoteCommand,
        writable::{ProtocolRead, ProtocolWrite},
    },
};
use tokio::{
    io::{self, AsyncRead, AsyncWriteExt},
    net::{TcpListener, tcp::OwnedWriteHalf},
    sync::Mutex,
};
use uuid::Uuid;

use super::zygote::ZygoteProcessConnection;

/// Correlary for Android's AMS central registries for processes. Provides exploit process communication and hacks around #4 (double Zygote spawns)
#[derive(Clone)]
pub struct ProcessManagementService {
    registry: ControllerRegistry,
    zygote_registrations: Arc<Mutex<HashMap<String, ZygoteProcessConnection>>>,
    /// Internal id to service name
    zygote_ids: Arc<Mutex<HashMap<Uuid, String>>>,
}

impl ProcessManagementService {
    pub async fn new(registry: ControllerRegistry) -> Result<Self> {
        let socket = TcpListener::bind(PMS_SOCKET_ADDRESS).await?;
        info!("PMS started");

        let pms = Self {
            registry,
            zygote_registrations: Default::default(),
            zygote_ids: Default::default(),
        };

        let inner_pms = pms.clone();
        tokio::spawn(async move {
            loop {
                match socket.accept().await {
                    Ok((stream, _)) => {
                        info!("Accepted PMS connection");
                        let mut pms_clone = inner_pms.clone();
                        tokio::spawn(async move {
                            let (mut stream_rx, stream_tx) = stream.into_split();
                            let stream_tx = Arc::new(Mutex::new(stream_tx));
                            // let mut is_first_command = true;
                            let mut connection = None;
                            loop {
                                match pms_clone
                                    .handle_command(
                                        &mut stream_rx,
                                        stream_tx.clone(),
                                        &mut connection,
                                    )
                                    .await
                                {
                                    Ok(Some(response)) => {
                                        let mut write_lock = stream_tx.lock().await;
                                        match response.write(&mut *write_lock).await {
                                            Ok(_) => {}
                                            Err(err) => {
                                                error!("Error responding to client: {err:?}");
                                                Self::transmit_kill_response(stream_tx.clone())
                                                    .await;
                                                return;
                                            }
                                        }
                                    }
                                    Err(err) => {
                                        if let Error::CommonError(
                                            pinitd_common::error::Error::IO(err),
                                        ) = &err
                                        {
                                            if err.kind() == io::ErrorKind::UnexpectedEof {
                                                if let Some(connection) = connection {
                                                    info!(
                                                        "Closing connection for {}",
                                                        connection.pinit_id
                                                    );
                                                }
                                                // Don't log eof errors; this is normal
                                                return;
                                            }
                                        }

                                        error!("Error handling client: {err:?}");
                                        Self::transmit_kill_response(stream_tx.clone()).await;
                                        return;
                                    }
                                    _ => info!("Dead case"),
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("Error accepting PMS connection: {}", e);
                    }
                }
            }
        });

        Ok(pms)
    }

    pub async fn register_spawn(&self, id: Uuid, service_name: String) {
        self.zygote_ids.lock().await.insert(id, service_name);
    }

    pub async fn clear_service(&self, name: &str) {
        let connection = self.zygote_registrations.lock().await.remove(name);
        if let Some(connection) = connection {
            self.zygote_ids.lock().await.remove(&connection.pinit_id);
        }
    }

    async fn handle_command<T>(
        &mut self,
        stream_rx: &mut T,
        stream_tx: Arc<Mutex<OwnedWriteHalf>>,
        connection: &mut Option<ZygoteProcessConnection>,
    ) -> Result<Option<PMSToRemoteCommand>>
    where
        T: AsyncRead + Unpin + Send,
    {
        let command = PMSFromRemoteCommand::read(stream_rx).await?;
        info!("Received PMS command: {:?}", command);

        let is_process_launch = matches!(command, PMSFromRemoteCommand::WrapperLaunched { .. });

        if connection.is_none() {
            if !is_process_launch {
                return Err(Error::Unknown("First command received on new PMS connection is not a WrapperLaunched command. Terminating".to_string()));
            }
        } else if is_process_launch {
            return Err(Error::Unknown(
                "Received WrapperLaunched command on existing PMS connection. Terminating"
                    .to_string(),
            ));
        }

        match command {
            PMSFromRemoteCommand::WrapperLaunched(pinit_id) => {
                let zygote_ids_lock = self.zygote_ids.lock().await;

                if let Some(service_name) = zygote_ids_lock.get(&pinit_id) {
                    let mut zygote_registration_lock = self.zygote_registrations.lock().await;

                    if zygote_registration_lock.contains_key(service_name) {
                        // This service has already registered a process. This is probably issue #4. Kill it
                        error!(
                            "Received duplicate process spawn for id {pinit_id}, service \"{service_name}\". Killing"
                        );
                        return Ok(Some(PMSToRemoteCommand::Kill));
                    }

                    let zygote_connection = ZygoteProcessConnection {
                        pinit_id,
                        service_name: service_name.clone(),
                        stream_tx,
                    };

                    // Register connection
                    zygote_registration_lock
                        .insert(service_name.clone(), zygote_connection.clone());
                    *connection = Some(zygote_connection);
                    info!("Registered \"{service_name}\" with id {pinit_id}");

                    return Ok(Some(PMSToRemoteCommand::AllowStart));
                } else {
                    // This is an unknown spawn. Kill it
                    return Ok(Some(PMSToRemoteCommand::Kill));
                }
            }
            PMSFromRemoteCommand::ProcessAttached(pid) => {
                let connection = connection.as_mut().unwrap();
                self.registry
                    .update_service_state(
                        connection.service_name.clone(),
                        ServiceRunState::Running { pid: Some(pid) },
                    )
                    .await?;
                info!("Received pid {pid} for \"{}\"", connection.service_name);

                Ok(Some(PMSToRemoteCommand::Ack))
            }
            PMSFromRemoteCommand::ProcessExited(_exit_code) => {
                // TODO: Implement
                Ok(None)
            }
        }
    }

    async fn transmit_kill_response(stream_tx: Arc<Mutex<OwnedWriteHalf>>) {
        let _ = stream_tx
            .lock()
            .await
            .write_all(&PMSToRemoteCommand::Kill.encode().unwrap());
    }
}
