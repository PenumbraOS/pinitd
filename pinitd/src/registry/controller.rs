use std::{
    collections::{HashMap, HashSet},
    hash::RandomState,
    io::ErrorKind,
    path::Path,
    sync::Arc,
    time::Duration,
};

use dependency_graph::{DependencyGraph, Step};
use file_lock::FileLock;
use pinitd_common::{
    CONFIG_DIR, ServiceRunState, ServiceStatus, UID, WorkerIdentity, ZYGOTE_READY_FILE,
    protocol::{CLICommand, CLIResponse},
    unit_config::ServiceConfig,
};
use tokio::{
    fs,
    sync::{Mutex, mpsc},
    time::{sleep, timeout},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    controller::{pms::ProcessManagementService, worker_manager::WorkerManager},
    error::{Error, Result},
    file::acquire_controller_lock,
    state::StoredState,
    types::Service,
    unit_parsing::ParsableServiceConfig,
    worker::protocol::{WorkerCommand, WorkerEvent, WorkerResponse},
};

use super::Registry;

#[derive(Clone)]
pub struct ControllerRegistry {
    controller_lock: Arc<Mutex<Option<FileLock>>>,
    pms: Option<Box<ProcessManagementService>>,
    services: Arc<Mutex<HashMap<String, Service>>>,
    stored_state: Arc<Mutex<StoredState>>,
    worker_manager: Arc<WorkerManager>,
    service_spawning_allowed: Arc<Mutex<bool>>,
    pending_autostart_services: Arc<Mutex<Option<Vec<String>>>>,
}

impl ControllerRegistry {
    pub async fn new(
        worker_event_tx: mpsc::Sender<WorkerEvent>,
        controller_lock: Arc<Mutex<Option<FileLock>>>,
    ) -> Result<Self> {
        let state = StoredState::load().await?;
        info!("Loaded enabled state for: {:?}", state.enabled_services);

        info!("Loading service configurations from {}", CONFIG_DIR);

        let worker_manager = Arc::new(WorkerManager::new(worker_event_tx));

        // Start the global worker listener
        worker_manager.start_listener().await?;

        let registry = Self {
            controller_lock,
            pms: None,
            services: Arc::new(Mutex::new(HashMap::new())),
            stored_state: Arc::new(Mutex::new(state)),
            worker_manager,
            service_spawning_allowed: Arc::new(Mutex::new(false)),
            pending_autostart_services: Arc::new(Mutex::new(None)),
        };

        Ok(registry)
    }

    // Helper methods for service access
    async fn with_service<F, R>(&self, name: &str, func: F) -> Result<R>
    where
        F: FnOnce(&Service) -> Result<R>,
    {
        let services = self.services.lock().await;
        let service = services
            .get(name)
            .ok_or_else(|| Error::UnknownServiceError(name.to_string()))?;
        func(service)
    }

    #[allow(dead_code)]
    async fn with_service_mut<F, R>(&self, name: &str, func: F) -> Result<R>
    where
        F: FnOnce(&mut Service) -> Result<R>,
    {
        let mut services = self.services.lock().await;
        let service = services
            .get_mut(name)
            .ok_or_else(|| Error::UnknownServiceError(name.to_string()))?;
        func(service)
    }

    async fn is_enabled(&self, name: &str) -> Result<bool> {
        let stored_state = self.stored_state.lock().await;
        Ok(stored_state.enabled(&name.to_string()))
    }

    async fn insert_service(&self, service: Service) -> Result<()> {
        let mut services = self.services.lock().await;
        services.insert(service.config().name.clone(), service);
        Ok(())
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
            .with_service(&name, |service| Ok(service.config().clone()))
            .await?;

        let new_config = self
            .load_unit_config(&existing_config.unit_file_path)
            .await?;

        if new_config != existing_config {
            let enabled = self.is_enabled(&name).await?;
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
            CLICommand::Start(name) => match self.service_start(name.clone(), false).await {
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

    pub async fn update_service_state(&self, name: String, state: ServiceRunState) -> Result<()> {
        let mut services = self.services.lock().await;
        if let Some(service) = services.get_mut(&name) {
            info!("Updating service state {name} with {state:?}");
            service.set_state(state);
        }
        Ok(())
    }

    async fn load_unit_config(&self, path: &Path) -> Result<ServiceConfig> {
        ServiceConfig::parse(path).await
    }

    async fn insert_unit_with_current_state(&mut self, config: ServiceConfig) -> Result<()> {
        let enabled = self.is_enabled(&config.name).await?;
        self.insert_unit(config, enabled).await
    }

    pub async fn service_start(&mut self, name: String, wait_for_start: bool) -> Result<bool> {
        {
            let spawning_allowed = self.service_spawning_allowed.lock().await;
            if !*spawning_allowed {
                return Err(Error::Unknown(
                    "Service spawning is not allowed as Zygote is not ready".to_string(),
                ));
            }
        }

        let allow_start = self
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
        self.start_dependencies(&name, wait_for_start).await?;

        let id = self.register_id(name.clone()).await;
        self.service_start_with_id(name, id, wait_for_start).await
    }

    pub async fn service_stop(&mut self, name: String) -> Result<()> {
        let config = self
            .with_service(&name, |service| Ok(service.config().clone()))
            .await?;

        let identity: WorkerIdentity = config.into();
        match self.worker_manager.get_worker_for_identity(&identity).await {
            Ok(connection) => {
                connection
                    .write_command(WorkerCommand::KillProcess {
                        service_name: name.clone(),
                    })
                    .await?;
            }
            Err(err) => error!("Cannot connect to worker to stop service \"{name}\": {err}"),
        }

        self.pms_stop(name).await;

        Ok(())
    }

    pub async fn service_restart(&mut self, name: String) -> Result<()> {
        // Simplified restart: stop then start
        info!("Restarting service \"{name}\"");
        self.service_stop(name.clone()).await?;
        self.pms_stop(name.clone()).await;
        self.service_start(name, false).await?;

        Ok(())
    }

    /// Set up worker processes, restoring existing ones if available. Returns true if this is a post-exploit controller
    pub async fn setup_workers(&self) -> Result<bool> {
        self.worker_manager.wait_for_worker_reconnections().await?;

        let connected_workers = self.worker_manager.all_workers().await;

        if !connected_workers.is_empty() {
            info!(
                "Received {} worker connections. This is a post-exploit controller. Locking Zygote spawns",
                connected_workers.len()
            );
            self.worker_manager.disable_spawning().await;

            let worker_identities = HashSet::<&WorkerIdentity, RandomState>::from_iter(
                connected_workers.iter().map(|worker| worker.identity()),
            );

            for config in self.all_autostart_configs().await? {
                let identity: WorkerIdentity = config.clone().into();
                if !worker_identities.contains(&identity) {
                    error!("Did not receive a connection from worker {identity:?}",)
                }
            }

            info!("Requesting current state from all workers...");
            for worker in connected_workers {
                match worker.request_current_state().await {
                    Ok(state) => {
                        info!(
                            "Received state from worker {:?}: {} services",
                            worker.identity(),
                            state.services.len()
                        );

                        for service_state in state.services {
                            self.update_service_state(
                                service_state.service_name,
                                service_state.run_state,
                            )
                            .await?;
                        }
                    }
                    Err(err) => {
                        warn!(
                            "Failed to get state from worker {:?}: {err}",
                            worker.identity()
                        );
                    }
                }
            }

            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn send_cgroup_reparent_command(&self, pid: usize) -> Result<()> {
        info!("Sending CGroupReparentCommand for PID {pid} to system worker");

        // We are always sending to system, so default se_info is sufficient
        let connection = self
            .worker_manager
            .get_worker_spawning_if_necessary(UID::System, None, None)
            .await?;

        let response = connection
            .write_command(WorkerCommand::CGroupReparentCommand { pid })
            .await?;

        match response {
            WorkerResponse::Success => {
                info!("CGroupReparentCommand executed successfully for PID {pid}");
                Ok(())
            }
            WorkerResponse::Error(err) => {
                error!("CGroupReparentCommand failed for PID {pid}: {err}");
                Err(Error::WorkerProtocolError(err))
            }
            _ => {
                let err = "Unexpected response to CGroupReparentCommand";
                error!("{err}");
                Err(Error::WorkerProtocolError(err.to_string()))
            }
        }
    }

    async fn handle_zygote_ready(&mut self) -> Result<()> {
        *self.service_spawning_allowed.lock().await = true;

        {
            self.unlock_controller_file_lock().await;

            if let Some(lock) = acquire_controller_lock() {
                *self.controller_lock.lock().await = Some(lock);
            }
        }

        let pending_services = self.pending_autostart_services.lock().await.take();
        if let Some(pending_services) = pending_services {
            info!(
                "Executing queued autostart for {} services in dependency order",
                pending_services.len()
            );
            for service_name in pending_services {
                info!("Starting queued service \"{service_name}\"");
                if let Err(err) = self
                    .service_start_internal(service_name.clone(), true)
                    .await
                {
                    error!("Failed to start queued service \"{service_name}\": {err}");
                }
            }
            info!("Queued autostart sequence complete.");
        }

        Ok(())
    }

    pub fn start_zygote_ready_watcher(&self) {
        let registry = self.clone();
        tokio::spawn(async move {
            info!("Polling for Zygote ready file");
            let _ = fs::remove_file(ZYGOTE_READY_FILE).await;

            // This is not inline with the timeout() call as Rust won't lint + format this expression if it is for some reason
            let watch_closure = async {
                loop {
                    // Check if the file exists
                    match fs::metadata(ZYGOTE_READY_FILE).await {
                        Ok(_) => {
                            info!("Zygote ready file detected, triggering Zygote ready handling");

                            if let Err(e) = fs::remove_file(ZYGOTE_READY_FILE).await {
                                warn!("Failed to remove Zygote ready file: {}", e);
                            }

                            if let Err(e) = registry.clone().handle_zygote_ready().await {
                                error!("Failed to handle Zygote ready: {}", e);
                            }

                            break;
                        }
                        Err(e) => {
                            // Expected error
                            match e.kind() {
                                ErrorKind::NotFound => {
                                    // File doesn't exist yet. This is normal
                                }
                                ErrorKind::PermissionDenied => {
                                    warn!(
                                        "Permission denied accessing zygote ready file path - filesystem may not be mounted"
                                    );
                                }
                                _ => {
                                    warn!("Error checking zygote ready file ({}): {}", e.kind(), e);
                                }
                            }
                        }
                    }

                    sleep(Duration::from_secs(1)).await;
                }
            };

            match timeout(Duration::from_secs(60), watch_closure).await {
                Ok(_) => {
                    info!("Zygote ready file polling completed successfully");
                }
                Err(_) => {
                    warn!("Zygote ready file polling timed out after 60 seconds");
                }
            }
        });
    }

    pub async fn unlock_controller_file_lock(&self) {
        let mut mutex = self.controller_lock.lock().await;
        // If we are still holding the lock, make sure to unlock before we grab a new lock
        if let Some(lock) = &*mutex {
            let _ = lock.unlock();
            *mutex = None;
        }
    }

    ///
    /// The dependency tree for all autostart configs, ordered with all dependencies before the services that require them.
    /// May contain duplicate configs
    ///
    pub async fn all_autostart_configs(&self) -> Result<Vec<ServiceConfig>> {
        // Build current list of registry in case it's mutated during iteration and to drop lock
        let service_names = self.service_names().await?;

        let mut autostart_services = Vec::new();
        for name in service_names {
            let should_start = self.service_can_autostart(name.clone()).await?;
            if should_start {
                autostart_services.push(name);
            }
        }

        if autostart_services.is_empty() {
            info!("No services to autostart");
            return Ok(Vec::new());
        }

        // Build service configs for dependency resolution
        // We can only autostart dependencies that are also autostart
        let mut all_autostart_service_configs = Vec::new();
        for service_name in &autostart_services {
            let config = self
                .with_service(service_name, |service| Ok(service.config().clone()))
                .await?;
            all_autostart_service_configs.push(config);
        }

        // Resolve dependency graph once and extract service names in dependency order
        let flattened_graph = DependencyGraph::from(&all_autostart_service_configs[..])
            .filter_map(|step| match step {
                Step::Resolved(service_config) => Some(service_config.clone()),
                Step::Unresolved(dep_name) => {
                    warn!("Unresolved dependency: \"{}\"", dep_name);
                    None
                }
            })
            .collect();
        Ok(flattened_graph)
    }

    pub async fn queue_autostart_all(&mut self, post_exploit: bool) -> Result<()> {
        let service_configs_to_start = self.all_autostart_configs().await?;

        info!(
            "Pre-spawning workers for {} total services (including dependencies)",
            service_configs_to_start.len()
        );

        if !post_exploit {
            for config in &service_configs_to_start {
                info!("Pre-spawning worker for UID {:?}", config.command.uid);
                if let Err(err) = self
                    .worker_manager
                    .get_worker_spawning_if_necessary(
                        config.command.uid.clone(),
                        config.se_info.clone(),
                        config.launch_package.clone(),
                    )
                    .await
                {
                    error!("Failed to prestart worker: {err}")
                }
            }
        }

        {
            let mut pending = self.pending_autostart_services.lock().await;
            *pending = Some(
                service_configs_to_start
                    .iter()
                    .map(|config| config.name.clone())
                    .collect(),
            );
        }

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

    async fn start_dependencies(&mut self, service_name: &str, wait_for_start: bool) -> Result<()> {
        let dependencies = self
            .with_service(service_name, |service| {
                Ok(service.config().dependencies.wants.clone())
            })
            .await?;

        for dep_name in dependencies {
            info!(
                "Starting dependency \"{}\" for service \"{}\"",
                dep_name, service_name
            );

            if let Err(_) = self.with_service(&dep_name, |_| Ok(())).await {
                warn!(
                    "Dependency \"{}\" not found for service \"{}\". Skipping",
                    dep_name, service_name
                );
                continue;
            }

            let is_running = self
                .with_service(&dep_name, |service| {
                    Ok(matches!(service.state(), ServiceRunState::Running { .. }))
                })
                .await?;

            if !is_running {
                if let Err(err) = self
                    .service_start_internal(dep_name.clone(), wait_for_start)
                    .await
                {
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

    async fn service_start_internal(&mut self, name: String, wait_for_start: bool) -> Result<bool> {
        let allow_start = self
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
        self.service_start_with_id(name, id, wait_for_start).await
    }
}

impl Registry for ControllerRegistry {
    async fn service_names(&self) -> Result<Vec<String>> {
        let services = self.services.lock().await;
        Ok(services.keys().cloned().collect())
    }

    async fn service_can_autostart(&self, name: String) -> Result<bool> {
        self.with_service(&name, |service| {
            Ok(service.enabled()
                && service.config().autostart
                && *service.state() == ServiceRunState::Stopped)
        })
        .await
    }

    async fn insert_unit(&mut self, config: ServiceConfig, enabled: bool) -> Result<()> {
        let service = Service::new(config, ServiceRunState::Stopped, enabled);
        self.insert_service(service).await?;
        Ok(())
    }

    async fn remove_unit(&mut self, name: String) -> Result<bool> {
        let _ = self.service_stop(name.clone()).await;

        let mut services = self.services.lock().await;
        let removed = services.remove(&name).is_some();

        Ok(removed)
    }

    async fn service_start_with_id(
        &mut self,
        name: String,
        id: Uuid,
        _wait_for_start: bool,
    ) -> Result<bool> {
        let config = self
            .with_service(&name, |service| Ok(service.config().clone()))
            .await?;

        let command = config.command.command_string().await?;
        let connection = self
            .worker_manager
            .get_worker_spawning_if_necessary(
                config.command.uid,
                config.se_info,
                config.launch_package,
            )
            .await?;

        let result = connection
            .write_command(WorkerCommand::SpawnProcess {
                command,
                pinit_id: id,
                service_name: name,
            })
            .await?;

        Ok(result == WorkerResponse::Success)
    }

    async fn service_enable(&self, name: String) -> Result<()> {
        let should_save = {
            let services = self.services.lock().await;
            if let Some(service) = services.get(&name) {
                if service.enabled() {
                    warn!("Attempted to enable already enabled service \"{name}\"");
                    false
                } else {
                    true
                }
            } else {
                return Err(Error::UnknownServiceError(name));
            }
        };

        if should_save {
            let mut services = self.services.lock().await;
            if let Some(service) = services.get(&name) {
                let new_service =
                    Service::new(service.config().clone(), service.state().clone(), true);
                services.insert(name.clone(), new_service);
            }
            drop(services);

            // Update stored state
            let mut stored_state = self.stored_state.lock().await;
            if !stored_state.enabled_services.iter().any(|s| *s == name) {
                stored_state.enabled_services.push(name);
            }
            let state = stored_state.clone();
            drop(stored_state);
            state.save().await?;
        }

        Ok(())
    }

    async fn service_disable(&self, name: String) -> Result<()> {
        let should_save = {
            let services = self.services.lock().await;
            if let Some(service) = services.get(&name) {
                if !service.enabled() {
                    warn!("Attempted to disable already disabled service \"{name}\"");
                    false
                } else {
                    true
                }
            } else {
                return Err(Error::UnknownServiceError(name));
            }
        };

        if should_save {
            // Update the service to be disabled
            let mut services = self.services.lock().await;
            if let Some(service) = services.get(&name) {
                let new_service =
                    Service::new(service.config().clone(), service.state().clone(), false);
                services.insert(name.clone(), new_service);
            }
            drop(services);

            // Update stored state
            let mut stored_state = self.stored_state.lock().await;
            if let Some(i) = stored_state
                .enabled_services
                .iter()
                .position(|s| *s == name)
            {
                stored_state.enabled_services.swap_remove(i);
            }
            let state = stored_state.clone();
            drop(stored_state);
            state.save().await?;
        }

        Ok(())
    }

    async fn service_status(&self, name: String) -> Result<ServiceStatus> {
        self.with_service(&name, |service| Ok(service.status()))
            .await
    }

    async fn service_list_all(&self) -> Result<Vec<ServiceStatus>> {
        let services = self.services.lock().await;
        Ok(services.values().map(|s| s.status()).collect())
    }

    async fn shutdown(&self) -> Result<()> {
        // Shutdown all workers using the worker manager
        self.worker_manager.shutdown_all().await
    }
}
