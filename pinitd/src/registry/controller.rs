use std::{path::Path, sync::Arc};

use pinitd_common::{
    CONFIG_DIR, ServiceRunState, ServiceStatus, UID,
    protocol::{CLICommand, CLIResponse},
};
use tokio::{
    fs,
    sync::{
        Mutex,
        broadcast::{self, Receiver, Sender},
    },
};
use tokio_util::sync::CancellationToken;

use crate::{
    error::{Error, Result},
    state::StoredState,
    types::{BaseService, Service},
    unit::ServiceConfig,
    worker::{
        connection::{WorkerConnection, WorkerConnectionStatus},
        protocol::{WorkerCommand, WorkerResponse},
    },
};

use super::{Registry, local::LocalRegistry};

enum ControllerRegistryWorker {
    Connected(WorkerConnection),
    Disconnected {
        status_tx: Sender<WorkerConnection>,
        status_rx: Receiver<WorkerConnection>,
    },
}

impl Clone for ControllerRegistryWorker {
    fn clone(&self) -> Self {
        match self {
            Self::Connected(arg0) => Self::Connected(arg0.clone()),
            Self::Disconnected {
                status_tx,
                status_rx,
            } => Self::Disconnected {
                status_tx: status_tx.clone(),
                status_rx: status_rx.resubscribe(),
            },
        }
    }
}

impl ControllerRegistryWorker {
    fn new_disconnected() -> Self {
        let (status_tx, status_rx) = broadcast::channel(10);
        ControllerRegistryWorker::Disconnected {
            status_tx,
            status_rx,
        }
    }
}

#[derive(Clone)]
pub struct ControllerRegistry {
    local: LocalRegistry,
    remote: Arc<Mutex<ControllerRegistryWorker>>,
}

impl ControllerRegistry {
    pub async fn new() -> Result<Self> {
        let state = StoredState::load().await?;
        info!("Loaded enabled state for: {:?}", state.enabled_services);

        info!("Loading service configurations from {}", CONFIG_DIR);
        let local = LocalRegistry::controller(state)?;
        Ok(Self {
            local,
            remote: Arc::new(Mutex::new(ControllerRegistryWorker::new_disconnected())),
        })
    }

    pub async fn load_from_disk(&self) -> Result<()> {
        let mut load_count = 0;

        let mut directory = fs::read_dir(CONFIG_DIR).await?;

        while let Some(entry) = directory.next_entry().await? {
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |ext| ext == "unit") {
                info!("Found config {}", path.display());
                match self.load_unit_config(&path).await {
                    Ok(config) => {
                        let name = config.name.clone();
                        match self.insert_unit_with_current_state(config).await {
                            Ok(_) => {
                                load_count += 1;
                            }
                            Err(err) => {
                                error!("Failed to insert unit file \"{name}\": {err}");
                            }
                        }
                    }
                    Err(err) => {
                        // Eat error
                        error!("Failed to load unit file \"{}\": {err}", path.display());
                    }
                }
            }
        }

        info!("Finished loading configurations. {load_count} services loaded.");

        Ok(())
    }

    pub async fn service_reload(&self, name: String) -> Result<Option<ServiceConfig>> {
        let existing_config = self
            .local
            .with_service(&name, |service| Ok(service.config().clone()))
            .await?;

        let new_config = self
            .load_unit_config(&existing_config.unit_file_path)
            .await?;

        if new_config != existing_config {
            let enabled = self.local.is_enabled(&name).await?;
            self.insert_unit(new_config.clone(), enabled).await?;
            if enabled {
                self.service_restart(name).await?;
            }

            Ok(Some(new_config))
        } else {
            Ok(None)
        }
    }

    pub async fn process_remote_command(
        &self,
        command: CLICommand,
        shutdown_token: CancellationToken,
    ) -> CLIResponse {
        match command {
            CLICommand::Start(name) => match self.service_start(name.clone()).await {
                Ok(did_start) => {
                    if did_start {
                        CLIResponse::Success(format!("Service \"{name}\" started",))
                    } else {
                        CLIResponse::Success(format!("Service \"{name}\" already running",))
                    }
                }
                Err(err) => {
                    CLIResponse::Error(format!("Failed to start service \"{name}\": {err}"))
                }
            },
            CLICommand::Stop(name) => match self.service_stop(name.clone()).await {
                // TODO: This says "stop initiated even if the service wasn't running"
                Ok(_) => CLIResponse::Success(format!("Service \"{name}\" stop initiated.")),
                Err(err) => CLIResponse::Error(format!("Failed to stop service \"{name}\": {err}")),
            },
            CLICommand::Restart(name) => match self.service_restart(name.clone()).await {
                Ok(_) => CLIResponse::Success(format!("Service \"{name}\" restarted")),
                Err(err) => {
                    CLIResponse::Error(format!("Failed to restart service \"{name}\": {err}"))
                }
            },
            CLICommand::Enable(name) => match self.service_enable(name.clone()).await {
                Ok(_) => CLIResponse::Success(format!("Service \"{name}\" enabled")),
                Err(err) => {
                    CLIResponse::Error(format!("Failed to enable service \"{name}\": {err}"))
                }
            },
            CLICommand::Disable(name) => match self.service_disable(name.clone()).await {
                Ok(_) => CLIResponse::Success(format!("Service \"{name}\" disabled")),
                Err(err) => {
                    CLIResponse::Error(format!("Failed to disable service \"{name}\": {err}"))
                }
            },
            CLICommand::Reload(name) => match self.service_reload(name.clone()).await {
                Ok(_) => CLIResponse::Success(format!("Service \"{name}\" reloaded")),
                Err(err) => {
                    CLIResponse::Error(format!("Failed to reload service \"{name}\": {err}"))
                }
            },
            CLICommand::Status(name) => match self.service_status(name).await {
                Ok(status) => CLIResponse::Status(status),
                Err(err) => CLIResponse::Error(err.to_string()),
            },
            CLICommand::List => match self.service_list_all().await {
                Ok(list) => CLIResponse::List(list),
                Err(err) => CLIResponse::Error(format!("Failed to retrieve service list: {err}")),
            },
            CLICommand::Shutdown => {
                info!("Shutdown RemoteCommand received.");
                shutdown_token.cancel();
                CLIResponse::ShuttingDown // Respond immediately
            }
        }
    }

    pub async fn update_worker_connection(&self, status: WorkerConnectionStatus) {
        let mut lock = self.remote.lock().await;

        match status {
            WorkerConnectionStatus::Connected(connection) => {
                let status_tx = match *lock {
                    ControllerRegistryWorker::Connected(_) => unreachable!(),
                    ControllerRegistryWorker::Disconnected { ref status_tx, .. } => {
                        status_tx.clone()
                    }
                };
                *lock = ControllerRegistryWorker::Connected(connection.clone());
                // Ensure lock is released
                drop(lock);
                let _ = status_tx.send(connection);
            }
            WorkerConnectionStatus::Disconnected => {
                *lock = ControllerRegistryWorker::new_disconnected();
            }
        }
    }

    pub async fn update_worker_service(&self, service: BaseService) -> Result<()> {
        // We don't want to write back to worker
        self.local
            .internal_insert_service(Service::from(service))
            .await
    }

    async fn remote_connection(&self) -> Result<WorkerConnection> {
        match self.remote.lock().await.clone() {
            ControllerRegistryWorker::Connected(connection) => Ok(connection),
            ControllerRegistryWorker::Disconnected { mut status_rx, .. } => {
                match status_rx.recv().await {
                    Ok(connection) => Ok(connection),
                    Err(err) => Err(Error::WorkerConnectionRecvError(err)),
                }
            }
        }
    }

    async fn load_unit_config(&self, path: &Path) -> Result<ServiceConfig> {
        ServiceConfig::parse(path).await
    }

    async fn insert_unit_with_current_state(&self, config: ServiceConfig) -> Result<()> {
        let enabled = self.local.is_enabled(&config.name).await?;
        self.insert_unit(config, enabled).await
    }
}

impl Registry for ControllerRegistry {
    async fn service_names(&self) -> Result<Vec<String>> {
        self.local.service_names().await
    }

    async fn service_can_autostart(&self, name: String) -> Result<bool> {
        self.local.service_can_autostart(name).await
    }

    async fn insert_unit(&self, config: ServiceConfig, enabled: bool) -> Result<()> {
        self.local.insert_unit(config.clone(), enabled).await?;

        if config.uid == UID::System {
            self.remote_connection()
                .await?
                .write_command(WorkerCommand::Create(config))
                .await?;
        }

        Ok(())
    }

    async fn remove_unit(&self, name: String) -> Result<bool> {
        let removed_local = self.local.remove_unit(name.clone()).await?;

        if removed_local && !self.local.is_shell_service(&name).await? {
            let response = self
                .remote_connection()
                .await?
                .write_command(WorkerCommand::Destroy(name))
                .await?;
            Ok(response == WorkerResponse::Success)
        } else {
            Ok(removed_local)
        }
    }

    async fn service_start(&self, name: String) -> Result<bool> {
        let allow_start = self
            .local
            .with_service(&name, |service| {
                if !service.enabled() {
                    warn!("Attempted to start disabled service \"{name}\". Ignoring.",);
                    return Err(Error::Unknown(format!("Service \"{name}\" is disabled.")));
                }

                Ok(!matches!(service.state(), ServiceRunState::Running { .. }))
            })
            .await?;

        if !allow_start {
            // Already running
            return Ok(false);
        }

        if self.local.is_shell_service(&name).await? {
            self.local.service_start(name).await
        } else {
            let result = self
                .remote_connection()
                .await?
                .write_command(WorkerCommand::Start(name))
                .await?;
            Ok(result == WorkerResponse::Success)
        }
    }

    async fn service_stop(&self, name: String) -> Result<()> {
        if self.local.is_shell_service(&name).await? {
            self.local.service_stop(name).await
        } else {
            self.remote_connection()
                .await?
                .write_command(WorkerCommand::Stop(name))
                .await?;
            Ok(())
        }
    }

    async fn service_restart(&self, name: String) -> Result<()> {
        if self.local.is_shell_service(&name).await? {
            self.local.service_restart(name).await
        } else {
            self.remote_connection()
                .await?
                .write_command(WorkerCommand::Restart(name))
                .await?;
            Ok(())
        }
    }

    async fn service_enable(&self, name: String) -> Result<()> {
        self.local.service_enable(name).await
    }

    async fn service_disable(&self, name: String) -> Result<()> {
        self.local.service_disable(name).await
    }

    async fn service_status(&self, name: String) -> Result<ServiceStatus> {
        self.local.service_status(name).await
    }

    async fn service_list_all(&self) -> Result<Vec<ServiceStatus>> {
        self.local.service_list_all().await
    }

    async fn shutdown(&self) -> Result<()> {
        let mut connection = self.remote_connection().await?;
        connection.shutdown().await;
        connection.write_command(WorkerCommand::Shutdown).await?;
        Ok(())
    }
}
