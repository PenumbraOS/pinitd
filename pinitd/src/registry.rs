use std::{
    collections::HashMap, future::ready, path::Path, process::Stdio, sync::Arc, time::Duration,
};

use nix::libc::{SIGTERM, kill};
use pinitd_common::{CONFIG_DIR, ServiceRunState, ServiceStatus};
use tokio::{
    fs,
    process::Command,
    sync::{Mutex, MutexGuard},
    task::JoinHandle,
    time::sleep,
};

use crate::{
    error::Error,
    state::StoredState,
    types::Service,
    unit::{RestartPolicy, ServiceConfig},
};

struct InnerServiceRegistry {
    stored_state: StoredState,
    registry: HashMap<String, Service>,
}

#[derive(Clone)]
pub struct ServiceRegistry(Arc<Mutex<InnerServiceRegistry>>);

impl ServiceRegistry {
    pub async fn load() -> Result<Self, Error> {
        let state = StoredState::load().await?;
        info!("Loaded enabled state for: {:?}", state.enabled_services);

        info!("Loading service configurations from {}", CONFIG_DIR);
        let mut directory = fs::read_dir(CONFIG_DIR).await?;

        let registry = HashMap::new();

        let inner = InnerServiceRegistry {
            stored_state: state,
            registry,
        };

        let registry = ServiceRegistry(Arc::new(Mutex::new(inner)));
        let mut load_count = 0;

        while let Some(entry) = directory.next_entry().await? {
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |ext| ext == "unit") {
                info!("Found config {}", path.display());
                // Eat errors
                if let Ok(_) = registry.load_unit(&path).await {
                    load_count += 1;
                }
            }
        }

        info!("Finished loading configurations. {load_count} services loaded.",);

        Ok(registry)
    }

    async fn with_registry<F, R>(&self, func: F) -> Result<R, Error>
    where
        F: FnOnce(MutexGuard<'_, InnerServiceRegistry>) -> Result<R, Error>,
    {
        self.with_registry_async(|registry| ready(func(registry)))
            .await
    }

    async fn with_registry_async<F, R, FR>(&self, func: F) -> Result<R, Error>
    where
        F: FnOnce(MutexGuard<'_, InnerServiceRegistry>) -> FR,
        FR: IntoFuture<Output = Result<R, Error>>,
    {
        let registry_lock = self.0.lock().await;
        let result = func(registry_lock).await?;
        Ok(result)
    }

    pub async fn with_service<F, R>(&self, name: &str, func: F) -> Result<R, Error>
    where
        F: FnOnce(&mut Service) -> Result<R, Error>,
    {
        let mut registry_lock = self.0.lock().await;
        let service = registry_lock
            .registry
            .get_mut(name)
            .ok_or_else(|| Error::UnknownServiceError(name.to_string()))?;
        let result = func(service)?;
        Ok(result)
    }

    async fn load_unit(&self, path: &Path) -> Result<bool, Error> {
        match ServiceConfig::parse(path).await {
            Ok(config) => {
                self.with_registry(|mut registry| {
                    let name = config.name.clone();
                    let service = registry.registry.get_mut(&name);
                    let enabled = service.as_ref().map_or(false, |s| s.enabled);
                    info!("Loaded config: {name} (Enabled: {enabled})");

                    if let Some(service) = service {
                        if config != service.config {
                            service.config = config;
                            Ok(true)
                        } else {
                            Ok(false)
                        }
                    } else {
                        let runtime = Service {
                            config,
                            state: ServiceRunState::Stopped,
                            enabled,
                            monitor_task: None,
                        };

                        registry.registry.insert(name, runtime);
                        Ok(true)
                    }
                })
                .await
            }
            Err(e) => {
                warn!("Failed to parse unit file {:?}: {}", path, e);
                Err(e)
            }
        }
    }

    pub async fn service_start(&self, name: String) -> Result<bool, Error> {
        let allow_start = self
            .with_service(&name, |service| {
                if !service.enabled {
                    warn!("Attempted to start disabled service \"{name}\". Ignoring.",);
                    return Err(Error::Unknown(format!("Service \"{name}\" is disabled.")));
                }

                Ok(!matches!(service.state, ServiceRunState::Running { .. }))
            })
            .await?;

        if !allow_start {
            // Already running
            return Ok(false);
        }

        let handle = self.spawn(name.clone());

        // Some time after start we should be able to acquire the lock to preserve this handle
        self.with_service(&name, |service| {
            service.monitor_task = Some(handle);

            Ok(true)
        })
        .await
    }

    pub async fn autostart_all(&self) -> Result<(), Error> {
        // Build current list of registry in case it's mutated during iteration and to drop lock
        let service_names = self
            .with_registry(|registry| {
                Ok(registry.registry.keys().cloned().collect::<Vec<String>>())
            })
            .await?;

        for name in service_names {
            // Each iteration of the loop reacquires a lock
            let should_start = self
                .with_service(&name, |service| {
                    Ok(service.enabled
                        && service.config.autostart
                        && service.state == ServiceRunState::Stopped)
                })
                .await?;

            if !should_start {
                continue;
            }

            info!("Autostarting service: {}", name);
            let _ = self.service_start(name.clone()).await;
        }

        info!("Autostart sequence complete.");

        Ok(())
    }

    pub async fn service_stop(&self, name: String) -> Result<(), Error> {
        self.with_service(&name, |service| Ok(service_stop_internal(&name, service)))
            .await
    }

    pub async fn service_restart(&self, name: String) -> Result<(), Error> {
        info!("Restarting service \"{name}\"");
        self.service_stop(name.clone()).await?;
        self.service_start(name).await?;

        Ok(())
    }

    pub async fn service_enable(&self, name: String) -> Result<(), Error> {
        let should_save = self
            .with_service(&name, |service| {
                if service.enabled {
                    warn!("Attempted to enable already enabled service \"{name}\"");
                    return Ok(false);
                }

                service.enabled = true;
                Ok(true)
            })
            .await?;

        if should_save {
            self.with_registry_async(|mut registry| {
                registry.stored_state.enabled_services.push(name);
                // Since it doesn't matter clone the state before saving for nicer async
                registry.stored_state.clone().save()
            })
            .await?;
        }

        Ok(())
    }

    pub async fn service_disable(&self, name: String) -> Result<(), Error> {
        let should_save = self
            .with_service(&name, |service| {
                if !service.enabled {
                    warn!("Attempted to disable already disabled service \"{name}\"");
                    return Ok(false);
                }

                service.enabled = false;
                Ok(true)
            })
            .await?;

        if should_save {
            self.with_registry_async(|mut registry| {
                if let Some(i) = registry
                    .stored_state
                    .enabled_services
                    .iter()
                    .position(|s| *s == name)
                {
                    registry.stored_state.enabled_services.swap_remove(i);
                }
                // Since it doesn't matter clone the state before saving for nicer async
                registry.stored_state.clone().save()
            })
            .await?;
        }

        Ok(())
    }

    pub async fn service_reload(&self, name: String) -> Result<(), Error> {
        let path = self
            .with_service(&name, |service| Ok(service.config.unit_file_path.clone()))
            .await?;

        let did_change = self.load_unit(&path).await?;
        if did_change {
            self.service_restart(name).await
        } else {
            Ok(())
        }
    }

    pub async fn service_status(&self, name: String) -> Result<ServiceStatus, Error> {
        self.with_service(&name, |service| Ok(service.status()))
            .await
    }

    pub async fn service_list_all(&self) -> Result<Vec<ServiceStatus>, Error> {
        self.with_registry(|registry| Ok(registry.registry.values().map(|s| s.status()).collect()))
            .await
    }

    pub async fn shutdown(&self) -> Result<(), Error> {
        self.with_registry_async(|mut registry| {
            for (name, service) in &mut registry.registry {
                service_stop_internal(name, service);
            }

            // Write last known state
            registry.stored_state.clone().save()
        })
        .await
    }

    fn spawn(&self, name: String) -> JoinHandle<()> {
        let inner_name = name.clone();
        let inner_registry = self.clone();
        tokio::spawn(async move {
            loop {
                info!("Starting process\"{name}\"");
                if let Ok((code, message)) =
                    inner_registry.perform_command(inner_name.clone()).await
                {
                    let mut expected_stop = false;

                    if let Ok(state) = inner_registry
                        .with_service(&inner_name, |service| Ok(service.state.clone()))
                        .await
                    {
                        // If we were in stopping, we expected the process to close. Don't mark it a failure
                        expected_stop = matches!(state, ServiceRunState::Stopping);
                    }

                    if !inner_registry
                        .stop_and_should_restart(
                            inner_name.clone(),
                            code != 0,
                            expected_stop,
                            message,
                        )
                        .await
                    {
                        // Terminate restart loop
                        return;
                    }

                    // Otherwise restart after delay
                    sleep(Duration::from_millis(1000)).await;
                } else {
                    // If error, terminate loop. It has already been logged
                    return;
                }
            }
        })
    }

    async fn perform_command(&self, name: String) -> Result<(i32, String), Error> {
        let mut registry_lock = self.0.lock().await;
        let service = registry_lock
            .registry
            .get_mut(&name)
            .ok_or_else(|| Error::Unknown(format!("Service \"{name}\" not found in registry")))?;

        let config = &service.config;

        info!("Spawning process for {name}: {}", config.command);

        let child = Command::new("sh")
            .args(&["-c", &config.command])
            // TODO: Auto pipe output to Android log?
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Make sure we clean up if we die
            .kill_on_drop(true)
            .spawn();

        match child {
            Ok(mut child) => {
                let pid = child.id().ok_or_else(|| {
                    Error::Unknown(format!("Failed to get PID for spawned process \"{name}\"",))
                })? as i32;
                info!("Service \"{name}\" spawned successfully with PID: {pid}",);

                service.state = ServiceRunState::Running { pid };

                // Drop contended resources before awaiting process
                drop(registry_lock);

                info!("Monitoring task started for service \"{name}\"");
                let (exit_code, exit_message) = match child.wait().await {
                    Ok(status) => {
                        info!("Process for service \"{name}\" exited with status: {status}",);
                        status.code().map_or_else(
                            || (127, "Exited via signal".to_string()),
                            |code| (code, format!("Exited with code {code}")),
                        )
                    }
                    Err(err) => {
                        error!("Error waiting on process for service \"{name}\": {err}");
                        (127, format!("Wait error: {err}"))
                    }
                };

                info!("Monitoring task finished for service \"{name}\"");

                Ok((exit_code, exit_message))
            }
            Err(err) => {
                let error_msg = format!("Failed to spawn process for \"{name}\": {err}");
                error!("{}", error_msg);
                service.state = ServiceRunState::Failed {
                    reason: error_msg.clone(),
                };
                Err(Error::Unknown(error_msg))
            }
        }
    }

    async fn stop_and_should_restart(
        &self,
        name: String,
        did_fail: bool,
        expected_stop: bool,
        exit_message: String,
    ) -> bool {
        self.with_service(&name, |service| {
            if did_fail && !expected_stop {
                warn!(
                    "Service \"{name}\" transitioned to Failed state with message {exit_message}"
                );
                service.state = ServiceRunState::Failed {
                    reason: exit_message.clone(),
                };
            } else {
                info!("Service \"{name}\" transitioned to Stopped state");
                service.state = ServiceRunState::Stopped;
            }

            if expected_stop {
                // Do not restart
                return Ok(false);
            }

            let should_restart = service.config.restart == RestartPolicy::Always
                || (did_fail && service.config.restart == RestartPolicy::OnFailure);
            if service.enabled && should_restart {
                warn!("Restarting service \"{name}\" due to exit: {exit_message}");

                return Ok(true);
            } else if !service.enabled {
                info!("Service \"{name}\" exited but is disabled, not restarting");
            } else {
                info!("Service \"{name}\" exited and restart is not configured");
            }

            Ok(false)
        })
        .await
        .map_or(false, |b| b)
    }
}

fn service_stop_internal(name: &str, service: &mut Service) {
    match &service.state {
        ServiceRunState::Running { pid } => {
            let pid = pid.clone();
            // Transition to stopping to mark this as an intentional service stop
            service.state = ServiceRunState::Stopping;

            info!("Attempting to stop service \"{name}\" (pid: {pid}). Sending SIGTERM");
            let result = unsafe { kill(pid, SIGTERM) };
            if result != 0 {
                warn!("Failed to send SIGTERM to pid {pid}: result {result}");
                // If we have a handle, attempt to kill via handle
                if let Some(handle) = &service.monitor_task {
                    handle.abort();
                }
                // Make sure handle drops
                service.monitor_task = None;
            }
        }
        _ => {
            warn!("Service \"{name}\" is not running");
        }
    }
}
