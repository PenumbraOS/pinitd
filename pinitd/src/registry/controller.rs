use std::{path::Path, sync::Arc};

use dependency_graph::{DependencyGraph, Step};
use pinitd_common::{
    CONFIG_DIR, ServiceRunState, ServiceStatus, UID,
    protocol::{CLICommand, CLIResponse},
    unit_config::ServiceConfig,
};
use tokio::{
    fs,
    sync::{
        Mutex,
        broadcast::{self, Receiver, Sender},
    },
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    controller::pms::ProcessManagementService,
    error::{Error, Result},
    state::StoredState,
    types::{BaseService, Service},
    unit_parsing::ParsableServiceConfig,
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
    pms: Option<Box<ProcessManagementService>>,
    local: LocalRegistry,
    remote: Arc<Mutex<ControllerRegistryWorker>>,
    use_system_domain: bool,
    disable_worker: bool,
}

impl ControllerRegistry {
    pub async fn new(
        connection: Option<WorkerConnection>,
        use_system_domain: bool,
        disable_worker: bool,
    ) -> Result<Self> {
        let state = StoredState::load().await?;
        info!("Loaded enabled state for: {:?}", state.enabled_services);

        info!("Loading service configurations from {}", CONFIG_DIR);
        let local = LocalRegistry::new_controller(state, use_system_domain)?;

        let connection = if let Some(connection) = connection {
            ControllerRegistryWorker::Connected(connection)
        } else {
            ControllerRegistryWorker::new_disconnected()
        };

        Ok(Self {
            pms: None,
            local,
            remote: Arc::new(Mutex::new(connection)),
            use_system_domain,
            disable_worker,
        })
    }

    pub async fn load_from_disk(&mut self) -> Result<()> {
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
        // TODO: Delete registry entries that aren't present

        info!("Finished loading configurations. {load_count} services loaded.");

        Ok(())
    }

    pub async fn set_pms(&mut self, pms: ProcessManagementService) {
        self.pms = Some(Box::new(pms));
    }

    pub async fn service_reload(&mut self, name: String) -> Result<Option<ServiceConfig>> {
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
        &mut self,
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
            CLICommand::ReloadAll => match self.load_from_disk().await {
                Ok(_) => CLIResponse::Success("Reloaded all services".into()),
                Err(err) => CLIResponse::Error(format!("Failed to reload all services: {err}")),
            },
            CLICommand::Config(name) => {
                match self
                    .local
                    .with_service(&name, |service| Ok(service.config().clone()))
                    .await
                {
                    Ok(config) => CLIResponse::Config(config),
                    Err(err) => {
                        CLIResponse::Error(format!("Failed to find service \"{name}\": {err}"))
                    }
                }
            }
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

    pub async fn update_service_state(&self, name: String, state: ServiceRunState) -> Result<()> {
        self.local
            .with_service_mut(&name, |service| {
                service.set_state(state);
                Ok(())
            })
            .await
    }

    async fn remote_connection(&self, allow_disconnected: bool) -> Result<WorkerConnection> {
        match self.remote.lock().await.clone() {
            ControllerRegistryWorker::Connected(connection) => Ok(connection),
            ControllerRegistryWorker::Disconnected { mut status_rx, .. } => {
                if allow_disconnected {
                    return Err(Error::WorkerProtocolError("Worker disconnected".into()));
                }

                match status_rx.recv().await {
                    Ok(connection) => Ok(connection),
                    Err(err) => Err(Error::WorkerConnectionRecvError(err)),
                }
            }
        }
    }

    async fn load_unit_config(&self, path: &Path) -> Result<ServiceConfig> {
        ServiceConfig::parse(path, self.local_service_uid()).await
    }

    async fn insert_unit_with_current_state(&mut self, config: ServiceConfig) -> Result<()> {
        let enabled = self.local.is_enabled(&config.name).await?;
        self.insert_unit(config, enabled).await
    }

    pub async fn service_start(&mut self, name: String) -> Result<bool> {
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

        // Start dependencies first (Wants dependencies)
        self.start_dependencies(&name).await?;

        let id = self.register_id(name.clone()).await;
        self.service_start_with_id(name, id).await
    }

    pub async fn service_stop(&mut self, name: String) -> Result<()> {
        if self.is_worker_service(&name).await? {
            self.remote_connection(true)
                .await?
                .write_command(WorkerCommand::Stop(name.clone()))
                .await?;
        } else {
            self.local.service_stop(name.clone()).await?;
        }

        self.pms_stop(name).await;

        Ok(())
    }

    pub async fn service_restart(&mut self, name: String) -> Result<()> {
        // TODO: Reimplement with pinit_id. Currently crashes
        if self.is_worker_service(&name).await? {
            let pinit_id = self.register_id(name.clone()).await;
            self.remote_connection(true)
                .await?
                .write_command(WorkerCommand::Restart {
                    service_name: name.clone(),
                    pinit_id,
                })
                .await?;
            self.pms_stop(name).await;
        } else {
            // Duplicate implementation as in local
            info!("Restarting service \"{name}\"");
            self.service_stop(name.clone()).await?;
            self.pms_stop(name.clone()).await;
            self.service_start(name).await?;
        }

        Ok(())
    }

    pub async fn autostart_all(&mut self) -> Result<()> {
        // Build current list of registry in case it's mutated during iteration and to drop lock
        let service_names = self.service_names().await?;

        let mut autostart_services = Vec::new();
        for name in service_names {
            let should_start = self.service_can_autostart(name.clone()).await?;
            if should_start {
                autostart_services.push(name);
            }
        }

        self.start_services_with_dependencies(autostart_services)
            .await?;

        info!("Autostart sequence complete.");

        Ok(())
    }

    async fn pms_stop(&self, name: String) {
        if let Some(pms) = &self.pms {
            pms.clear_service(&name).await;
        }
    }

    async fn register_id(&mut self, name: String) -> Uuid {
        let id = Uuid::new_v4();
        info!("Registering id {id} for \"{name}\"");
        self.pms
            .as_mut()
            .unwrap()
            .register_spawn(id.clone(), name.clone())
            .await;

        id
    }

    async fn start_dependencies(&mut self, service_name: &str) -> Result<()> {
        let dependencies = self
            .local
            .with_service(service_name, |service| {
                Ok(service.config().dependencies.wants.clone())
            })
            .await?;

        for dep_name in dependencies {
            info!(
                "Starting dependency \"{}\" for service \"{}\"",
                dep_name, service_name
            );

            if let Err(_) = self.local.with_service(&dep_name, |_| Ok(())).await {
                warn!(
                    "Dependency \"{}\" not found for service \"{}\". Skipping",
                    dep_name, service_name
                );
                continue;
            }

            let is_running = self
                .local
                .with_service(&dep_name, |service| {
                    Ok(matches!(service.state(), ServiceRunState::Running { .. }))
                })
                .await?;

            if !is_running {
                if let Err(err) = self.service_start_internal(dep_name.clone()).await {
                    warn!(
                        "Failed to start dependency \"{}\" for service \"{}\": {}",
                        dep_name, service_name, err
                    );
                }
            } else {
                info!("Dependency \"{}\" is already running", dep_name);
            }
        }

        Ok(())
    }

    async fn service_start_internal(&mut self, name: String) -> Result<bool> {
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

        let id = self.register_id(name.clone()).await;
        self.service_start_with_id(name, id).await
    }

    async fn start_services_with_dependencies(&mut self, service_names: Vec<String>) -> Result<()> {
        let mut service_configs = Vec::new();
        for name in &service_names {
            let config = self
                .local
                .with_service(name, |service| Ok(service.config().clone()))
                .await?;
            service_configs.push(config);
        }

        let dependency_graph = DependencyGraph::from(&service_configs[..]);

        // Start services in dependency order
        for step in dependency_graph {
            match step {
                Step::Resolved(service_config) => {
                    info!("Autostarting service: \"{}\"", service_config.name);
                    let _ = self
                        .service_start_internal(service_config.name.clone())
                        .await;
                }
                Step::Unresolved(dep_name) => {
                    warn!("Unresolved dependency: \"{}\"", dep_name);
                }
            }
        }

        Ok(())
    }

    async fn is_worker_service(&self, name: &str) -> Result<bool> {
        if self.disable_worker {
            return Ok(false);
        }

        self.local.is_worker_service(name).await
    }
}

impl Registry for ControllerRegistry {
    async fn service_names(&self) -> Result<Vec<String>> {
        self.local.service_names().await
    }

    async fn service_can_autostart(&self, name: String) -> Result<bool> {
        self.local.service_can_autostart(name).await
    }

    async fn insert_unit(&mut self, config: ServiceConfig, enabled: bool) -> Result<()> {
        self.local.insert_unit(config.clone(), enabled).await?;

        if config.command.uid == self.worker_service_uid() {
            self.remote_connection(true)
                .await?
                .write_command(WorkerCommand::Create(config))
                .await?;
        }

        Ok(())
    }

    async fn remove_unit(&mut self, name: String) -> Result<bool> {
        let removed_local = self.local.remove_unit(name.clone()).await?;

        if removed_local && self.is_worker_service(&name).await? {
            let response = self
                .remote_connection(true)
                .await?
                .write_command(WorkerCommand::Destroy(name))
                .await?;
            Ok(response == WorkerResponse::Success)
        } else {
            Ok(removed_local)
        }
    }

    async fn service_start_with_id(&mut self, name: String, id: Uuid) -> Result<bool> {
        if self.is_worker_service(&name).await? {
            let result = self
                .remote_connection(true)
                .await?
                .write_command(WorkerCommand::Start {
                    service_name: name,
                    pinit_id: id,
                })
                .await?;
            Ok(result == WorkerResponse::Success)
        } else {
            self.local.service_start_with_id(name, id).await
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
        let mut connection = self.remote_connection(true).await?;
        connection.shutdown().await;
        connection.write_command(WorkerCommand::Shutdown).await?;
        Ok(())
    }

    fn local_service_uid(&self) -> UID {
        if self.use_system_domain {
            UID::System
        } else {
            UID::Shell
        }
    }

    fn worker_service_uid(&self) -> UID {
        if self.use_system_domain {
            UID::Shell
        } else {
            UID::System
        }
    }
}
