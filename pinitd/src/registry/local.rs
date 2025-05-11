use std::{collections::HashMap, future::ready, process::Stdio, sync::Arc, time::Duration};

use nix::libc::{SIGTERM, kill};
use pinitd_common::{ServiceRunState, ServiceStatus, UID};
use tokio::{
    process::Command,
    sync::{Mutex, MutexGuard},
    task::JoinHandle,
    time::sleep,
};

use crate::{
    error::{Error, Result},
    state::StoredState,
    types::{Service, SyncedService},
    unit::{RestartPolicy, ServiceConfig},
    worker::connection::ControllerConnection,
};

use super::Registry;

struct InnerServiceRegistry {
    stored_state: StoredState,
    registry: HashMap<String, Service>,
    controller_connection: Option<ControllerConnection>,
}

#[derive(Clone)]
pub struct LocalRegistry(Arc<Mutex<InnerServiceRegistry>>);

impl LocalRegistry {
    pub fn controller(stored_state: StoredState) -> Result<Self> {
        let inner = InnerServiceRegistry {
            stored_state,
            registry: HashMap::new(),
            controller_connection: None,
        };

        let registry = LocalRegistry(Arc::new(Mutex::new(inner)));
        Ok(registry)
    }

    pub fn worker(connection: ControllerConnection) -> Result<Self> {
        let inner = InnerServiceRegistry {
            stored_state: StoredState::dummy(),
            registry: HashMap::new(),
            controller_connection: Some(connection),
        };

        let registry = LocalRegistry(Arc::new(Mutex::new(inner)));
        Ok(registry)
    }

    async fn with_registry<F, R>(&self, func: F) -> Result<R>
    where
        F: FnOnce(MutexGuard<'_, InnerServiceRegistry>) -> Result<R>,
    {
        self.with_registry_async(|registry| ready(func(registry)))
            .await
    }

    async fn with_registry_async<F, R, FR>(&self, func: F) -> Result<R>
    where
        F: FnOnce(MutexGuard<'_, InnerServiceRegistry>) -> FR,
        FR: IntoFuture<Output = Result<R>>,
    {
        let registry_lock = self.0.lock().await;
        let result = func(registry_lock).await?;
        Ok(result)
    }

    pub async fn with_service<F, R>(&self, name: &str, func: F) -> Result<R>
    where
        F: FnOnce(&Service) -> Result<R>,
    {
        let registry_lock = self.0.lock().await;
        let service = registry_lock
            .registry
            .get(name)
            .ok_or_else(|| Error::UnknownServiceError(name.to_string()))?;
        let result = func(service)?;
        Ok(result)
    }

    pub async fn with_service_mut<F, R>(&self, name: &str, func: F) -> Result<R>
    where
        F: FnOnce(&mut SyncedService) -> Result<R>,
    {
        let mut registry_lock = self.0.lock().await;
        let connection = registry_lock.controller_connection.clone();
        let service = registry_lock
            .registry
            .get_mut(name)
            .ok_or_else(|| Error::UnknownServiceError(name.to_string()))?;
        let mut service = SyncedService::from(service, connection);
        let result = func(&mut service)?;
        // Don't await update; we want it to not block current command
        let _ = service.send_update_if_necessary();
        Ok(result)
    }

    pub async fn is_enabled(&self, name: &String) -> Result<bool> {
        self.with_registry(|registry| Ok(registry.stored_state.enabled(name)))
            .await
    }

    pub async fn is_shell_service(&self, name: &str) -> Result<bool> {
        self.with_service(name, |service| {
            info!("Config {:?}", service.config());
            Ok(service.config().uid == UID::Shell)
        })
        .await
    }

    pub async fn internal_insert_service(&self, service: Service) -> Result<()> {
        self.with_registry(|mut registry| {
            registry
                .registry
                .insert(service.config().name.clone(), service);
            Ok(())
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
                        .with_service(&inner_name, |service| Ok(service.state().clone()))
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

    async fn perform_command(&self, name: String) -> Result<(i32, String)> {
        let config = self
            .with_service(&name, |service| Ok(service.config().clone()))
            .await?;

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

                self.with_service_mut(&name, |service| {
                    service.set_state(ServiceRunState::Running { pid });
                    Ok(())
                })
                .await?;

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
                self.with_service_mut(&name, |service| {
                    service.set_state(ServiceRunState::Failed {
                        reason: error_msg.clone(),
                    });
                    Ok(())
                })
                .await?;
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
        self.with_service_mut(&name, |service| {
            if did_fail && !expected_stop {
                warn!(
                    "Service \"{name}\" transitioned to Failed state with message {exit_message}"
                );
                service.set_state(ServiceRunState::Failed {
                    reason: exit_message.clone(),
                });
            } else {
                info!("Service \"{name}\" transitioned to Stopped state");
                service.set_state(ServiceRunState::Stopped);
            }

            if expected_stop {
                // Do not restart
                return Ok(false);
            }

            let should_restart = service.config().restart == RestartPolicy::Always
                || (did_fail && service.config().restart == RestartPolicy::OnFailure);
            if service.enabled() && should_restart {
                warn!("Restarting service \"{name}\" due to exit: {exit_message}");

                return Ok(true);
            } else if !service.enabled() {
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

impl Registry for LocalRegistry {
    async fn service_names(&self) -> Result<Vec<String>> {
        self.with_registry(|registry| {
            Ok(registry.registry.keys().cloned().collect::<Vec<String>>())
        })
        .await
    }

    async fn service_can_autostart(&self, name: String) -> Result<bool> {
        self.with_service(&name, |service| {
            Ok(service.enabled()
                && service.config().autostart
                && *service.state() == ServiceRunState::Stopped)
        })
        .await
    }

    async fn insert_unit(&self, config: ServiceConfig, enabled: bool) -> Result<()> {
        let service = Service::new(config, ServiceRunState::Stopped, enabled);
        self.internal_insert_service(service).await
    }

    async fn remove_unit(&self, name: String) -> Result<bool> {
        self.service_stop(name.clone()).await?;
        self.with_registry(|mut registry| {
            let success = registry.registry.remove(&name).is_some();
            Ok(success)
        })
        .await
    }

    async fn service_start(&self, name: String) -> Result<bool> {
        let handle = self.spawn(name.clone());

        // Some time after start we should be able to acquire the lock to preserve this handle
        self.with_service_mut(&name, |service| {
            service.set_monitor_task(Some(handle));

            Ok(true)
        })
        .await
    }

    async fn service_stop(&self, name: String) -> Result<()> {
        self.with_service_mut(&name, |service| Ok(service_stop_internal(&name, service)))
            .await
    }

    async fn service_restart(&self, name: String) -> Result<()> {
        info!("Restarting service \"{name}\"");
        self.service_stop(name.clone()).await?;
        self.service_start(name).await?;

        Ok(())
    }

    async fn service_enable(&self, name: String) -> Result<()> {
        let should_save = self
            .with_service_mut(&name, |service| {
                if service.enabled() {
                    warn!("Attempted to enable already enabled service \"{name}\"");
                    return Ok(false);
                }

                service.set_enabled(true);
                Ok(true)
            })
            .await?;

        // TODO: Use enable_service
        if should_save {
            let state = self
                .with_registry(|mut registry| {
                    if !registry
                        .stored_state
                        .enabled_services
                        .iter()
                        .find(|s| **s == name)
                        .is_some()
                    {
                        // Service is not already enabled
                        registry.stored_state.enabled_services.push(name);
                    }
                    // Since it doesn't matter clone the state before saving for nicer async
                    Ok(registry.stored_state.clone())
                })
                .await?;

            state.save().await?;
        }

        Ok(())
    }

    async fn service_disable(&self, name: String) -> Result<()> {
        // TODO: Use disable_service
        let should_save = self
            .with_service_mut(&name, |service| {
                if !service.enabled() {
                    warn!("Attempted to disable already disabled service \"{name}\"");
                    return Ok(false);
                }

                service.set_enabled(false);
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

    async fn service_status(&self, name: String) -> Result<ServiceStatus> {
        self.with_service(&name, |service| Ok(service.status()))
            .await
    }

    async fn service_list_all(&self) -> Result<Vec<ServiceStatus>> {
        self.with_registry(|registry| Ok(registry.registry.values().map(|s| s.status()).collect()))
            .await
    }

    async fn shutdown(&self) -> Result<()> {
        self.with_registry_async(|mut registry| {
            for (name, service) in &mut registry.registry {
                // We don't need to send anything at shutdown
                let mut service = SyncedService::from(service, None);
                service_stop_internal(name, &mut service);
            }

            // Write last known state
            registry.stored_state.clone().save()
        })
        .await
    }
}

fn service_stop_internal(name: &str, service: &mut SyncedService) {
    match &service.state() {
        ServiceRunState::Running { pid } => {
            let pid = pid.clone();
            // Transition to stopping to mark this as an intentional service stop
            service.set_state(ServiceRunState::Stopping);

            info!("Attempting to stop service \"{name}\" (pid: {pid}). Sending SIGTERM");
            let result = unsafe { kill(pid, SIGTERM) };
            if result != 0 {
                warn!("Failed to send SIGTERM to pid {pid}: result {result}");
            } else {
                info!("SIGTERM succeeded on pid {pid}");
            }
            // If we have a handle, attempt to kill via handle
            if let Some(handle) = service.monitor_task() {
                handle.abort();
            }
            // Make sure handle drops
            service.set_monitor_task(None);
        }
        _ => {
            warn!("Service \"{name}\" is not running");
        }
    }
}
