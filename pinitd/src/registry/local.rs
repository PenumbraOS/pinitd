use std::{collections::HashMap, future::ready, sync::Arc, time::Duration};

use nix::libc::{SIGTERM, kill};
use pinitd_common::{
    ServiceRunState, ServiceStatus, UID,
    unit_config::{RestartPolicy, ServiceConfig},
};
use tokio::{
    sync::{Mutex, MutexGuard, oneshot},
    task::JoinHandle,
    time::sleep,
};
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    registry::spawn::SpawnCommand,
    state::StoredState,
    types::{Service, SyncedService},
    worker::connection::ControllerConnection,
};

use super::Registry;

struct InnerServiceRegistry {
    stored_state: StoredState,
    registry: HashMap<String, Service>,
    controller_connection: ControllerConnection,
}

#[derive(Clone)]
pub struct LocalRegistry {
    inner: Arc<Mutex<InnerServiceRegistry>>,
    use_system_domain: bool,
    zygote_lock: Arc<Mutex<()>>,
}

impl LocalRegistry {
    pub fn new_controller(stored_state: StoredState, use_system_domain: bool) -> Result<Self> {
        let inner = InnerServiceRegistry {
            stored_state,
            registry: HashMap::new(),
            controller_connection: ControllerConnection::Disabled,
        };

        let registry = LocalRegistry {
            inner: Arc::new(Mutex::new(inner)),
            use_system_domain,
            zygote_lock: Arc::new(Mutex::new(())),
        };
        Ok(registry)
    }

    pub fn new_worker(connection: ControllerConnection, use_shell_domain: bool) -> Result<Self> {
        let inner = InnerServiceRegistry {
            stored_state: StoredState::dummy(),
            registry: HashMap::new(),
            controller_connection: connection,
        };

        let registry = LocalRegistry {
            inner: Arc::new(Mutex::new(inner)),
            use_system_domain: !use_shell_domain,
            zygote_lock: Arc::new(Mutex::new(())),
        };
        Ok(registry)
    }

    async fn is_worker(&self) -> bool {
        matches!(
            self.inner.lock().await.controller_connection,
            ControllerConnection::WithConnection(_)
        )
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
        let registry_lock = self.inner.lock().await;
        let result = func(registry_lock).await?;
        Ok(result)
    }

    pub async fn with_service<F, R>(&self, name: &str, func: F) -> Result<R>
    where
        F: FnOnce(&Service) -> Result<R>,
    {
        let registry_lock = self.inner.lock().await;
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
        let mut registry_lock = self.inner.lock().await;
        let connection = registry_lock.controller_connection.clone();
        let service = registry_lock
            .registry
            .get_mut(name)
            .ok_or_else(|| Error::UnknownServiceError(name.to_string()))?;
        let mut service = SyncedService::from(service, connection);
        let result = func(&mut service)?;
        let service = service.sendable();
        // Don't await update; we want it to not block current command
        tokio::spawn(async move {
            let _ = service.send_update_if_necessary().await;
        });
        Ok(result)
    }

    pub async fn is_enabled(&self, name: &String) -> Result<bool> {
        self.with_registry(|registry| Ok(registry.stored_state.enabled(name)))
            .await
    }

    pub async fn is_worker_service(&self, name: &str) -> Result<bool> {
        self.with_service(name, |service| {
            Ok(service.config().command.uid == self.worker_service_uid())
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

    pub async fn service_stop(&mut self, name: String) -> Result<()> {
        self.with_service_mut(&name, |service| Ok(service_stop_internal(&name, service)))
            .await
    }

    pub async fn service_restart_with_id(&mut self, name: String, pinit_id: Uuid) -> Result<()> {
        info!("Restarting service \"{name}\"");
        self.service_stop(name.clone()).await?;
        self.service_start_with_id(name, pinit_id, false).await?;

        Ok(())
    }

    async fn spawn(
        &self,
        name: String,
        pinit_id: Uuid,
        force_zygote_spawn: bool,
        wait_for_spawn: bool,
    ) -> JoinHandle<()> {
        let inner_name = name.clone();
        let inner_registry = self.clone();
        let (spawn_complete_tx, spawn_complete_rx) = oneshot::channel::<()>();
        let mut optional_spawn_complete_tx = Some(spawn_complete_tx);

        let zygote_lock = self.zygote_lock.clone();

        let watcher_handle = tokio::spawn(async move {
            loop {
                info!("Starting process \"{name}\"");
                let spawn_result = SpawnCommand::spawn(
                    inner_registry.clone(),
                    inner_name.clone(),
                    pinit_id,
                    force_zygote_spawn,
                    zygote_lock.clone(),
                    optional_spawn_complete_tx,
                )
                .await;

                optional_spawn_complete_tx = None;

                if let Ok(SpawnCommand {
                    exit_code,
                    exit_message,
                }) = spawn_result
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
                            exit_code != 0,
                            expected_stop,
                            exit_message,
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
        });

        // Wait for initial spawn event
        if wait_for_spawn {
            let _ = spawn_complete_rx.await;
        }

        watcher_handle
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

    async fn insert_unit(&mut self, config: ServiceConfig, enabled: bool) -> Result<()> {
        let service = Service::new(config, ServiceRunState::Stopped, enabled);
        self.internal_insert_service(service).await
    }

    async fn remove_unit(&mut self, name: String) -> Result<bool> {
        self.service_stop(name.clone()).await?;
        self.with_registry(|mut registry| {
            let success = registry.registry.remove(&name).is_some();
            Ok(success)
        })
        .await
    }

    async fn service_start_with_id(
        &mut self,
        name: String,
        id: Uuid,
        wait_for_start: bool,
    ) -> Result<bool> {
        // If we're not a worker process, but we have this marked as a worker service, we can't spawn it without going through Zygote
        let force_zygote_spawn =
            !self.is_worker().await && self.is_worker_service(&name).await.map_or(false, |b| b);
        let handle = self
            .spawn(name.clone(), id, force_zygote_spawn, wait_for_start)
            .await;

        // Some time after start we should be able to acquire the lock to preserve this handle
        self.with_service_mut(&name, |service| {
            service.set_monitor_task(Some(handle));

            Ok(true)
        })
        .await
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
                let mut service = SyncedService::from(service, ControllerConnection::Disabled);
                service_stop_internal(name, &mut service);
            }

            // Write last known state
            registry.stored_state.clone().save()
        })
        .await
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

fn service_stop_internal(name: &str, service: &mut SyncedService) {
    match &service.state() {
        ServiceRunState::Running { pid } => {
            let pid = pid.clone();
            // Transition to stopping to mark this as an intentional service stop
            service.set_state(ServiceRunState::Stopping);

            if let Some(pid) = pid {
                info!("Attempting to stop service \"{name}\" (PID: {pid}). Sending SIGTERM");
                let result = unsafe { kill(pid as i32, SIGTERM) };
                if result != 0 {
                    warn!("Failed to send SIGTERM to pid {pid}: result {result}");
                } else {
                    info!("SIGTERM succeeded on pid {pid}");
                }
            } else {
                info!("Attempting to stop service \"{name}\" with unknown PID");
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
