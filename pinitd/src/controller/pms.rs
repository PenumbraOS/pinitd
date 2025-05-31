use std::{collections::HashMap, sync::Arc};

use crate::{
    error::{Error, Result},
    registry::controller::ControllerRegistry,
};
use pinitd_common::{
    PMS_SOCKET_ADDRESS, ServiceRunState,
    bincode::Bincodable,
    protocol::{PMSFromRemoteCommand, PMSToRemoteCommand, writable::ProtocolRead},
};
use tokio::{
    io::{AsyncRead, AsyncWriteExt},
    net::{TcpListener, tcp::OwnedWriteHalf},
    sync::Mutex,
};

use super::zygote::ZygoteProcessConnection;

/// Correlary for Android's AMS central registries for processes. Provides exploit process communication and hacks around #4 (double Zygote spawns)
#[derive(Clone)]
pub struct ProcessManagementService {
    registry: ControllerRegistry,
    zygote_registrations: Arc<Mutex<HashMap<String, ZygoteProcessConnection>>>,
    /// Internal id to service name
    zygote_ids: Arc<Mutex<HashMap<u32, String>>>,
}

impl ProcessManagementService {
    pub async fn spawn(registry: ControllerRegistry) -> Result<Self> {
        let socket = TcpListener::bind(PMS_SOCKET_ADDRESS).await?;

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
                            let mut is_first_command = true;
                            loop {
                                match pms_clone
                                    .handle_command(
                                        &mut stream_rx,
                                        stream_tx.clone(),
                                        &mut is_first_command,
                                    )
                                    .await
                                {
                                    Ok(Some(response)) => match response.encode() {
                                        Ok(data) => {
                                            let _ = stream_tx.lock().await.write_all(&data).await;
                                        }
                                        Err(err) => {
                                            error!("Error responding to client: {err:?}");
                                            Self::transmit_kill_response(stream_tx.clone()).await;
                                            return;
                                        }
                                    },
                                    Err(err) => {
                                        error!("Error handling client: {err:?}");
                                        Self::transmit_kill_response(stream_tx.clone()).await;
                                        return;
                                    }
                                    _ => {}
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

    async fn handle_command<T>(
        &mut self,
        stream_rx: &mut T,
        stream_tx: Arc<Mutex<OwnedWriteHalf>>,
        is_first_command: &mut bool,
    ) -> Result<Option<PMSToRemoteCommand>>
    where
        T: AsyncRead + Unpin + Send,
    {
        let command = PMSFromRemoteCommand::read(stream_rx).await?;
        info!("Received PMS command: {:?}", command);

        let is_process_launch = matches!(command, PMSFromRemoteCommand::ProcessLaunched { .. });

        if *is_first_command {
            if !is_process_launch {
                return Err(Error::Unknown("First command received on new PMS connection is not a launch command. Terminating".to_string()));
            }
        } else if is_process_launch {
            return Err(Error::Unknown(
                "Received process launch command on existing PMS connection. Terminating"
                    .to_string(),
            ));
        }

        match command {
            PMSFromRemoteCommand::ProcessLaunched { pinit_id, pid } => {
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

                    // Register connection
                    zygote_registration_lock
                        .insert(service_name.clone(), ZygoteProcessConnection { stream_tx });

                    self.registry
                        .update_service_state(
                            service_name.clone(),
                            ServiceRunState::Running { pid },
                        )
                        .await?;

                    return Ok(Some(PMSToRemoteCommand::AllowStart));
                } else {
                    // This is an unknown spawn. Kill it
                    return Ok(Some(PMSToRemoteCommand::Kill));
                }
            }
        }

        Ok(None)
    }

    async fn transmit_kill_response(stream_tx: Arc<Mutex<OwnedWriteHalf>>) {
        let _ = stream_tx
            .lock()
            .await
            .write_all(&PMSToRemoteCommand::Kill.encode().unwrap());
    }
}
