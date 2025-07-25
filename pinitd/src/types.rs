use pinitd_common::{ServiceRunState, ServiceStatus, unit_config::ServiceConfig};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::{
    error::Result,
    worker::{connection::ControllerConnection, protocol::WorkerResponse},
};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct BaseService {
    pub config: ServiceConfig,
    pub state: ServiceRunState,
    pub enabled: bool,
}

pub struct Service {
    inner: BaseService,
    monitor_task: Option<JoinHandle<()>>,
}

impl Clone for Service {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            monitor_task: None,
        }
    }
}

impl Service {
    pub fn new(config: ServiceConfig, state: ServiceRunState, enabled: bool) -> Self {
        Self {
            inner: BaseService {
                config,
                state,
                enabled,
            },
            monitor_task: None,
        }
    }

    pub fn from(service: BaseService) -> Self {
        Self {
            inner: service,
            monitor_task: None,
        }
    }

    pub fn status(&self) -> ServiceStatus {
        ServiceStatus {
            name: self.inner.config.name.clone(),
            uid: self.inner.config.command.uid.clone(),
            enabled: self.inner.enabled,
            state: self.inner.state.clone(),
            config_path: self.inner.config.unit_file_path.clone(),
        }
    }

    pub fn config(&self) -> &ServiceConfig {
        &self.inner.config
    }

    pub fn enabled(&self) -> bool {
        self.inner.enabled
    }

    pub fn state(&self) -> &ServiceRunState {
        &self.inner.state
    }
}

pub struct SyncedService<'a> {
    service: &'a mut Service,
    did_change: bool,
    connection: ControllerConnection,
}

impl<'a> SyncedService<'a> {
    pub fn from(service: &'a mut Service, connection: ControllerConnection) -> Self {
        SyncedService {
            service,
            did_change: false,
            connection,
        }
    }

    pub fn sendable(self) -> SendableService {
        SendableService {
            service: self.service.clone(),
            did_change: self.did_change,
            connection: self.connection,
        }
    }

    #[allow(dead_code)]
    pub fn set_config(&mut self, config: ServiceConfig) {
        if config != self.service.inner.config {
            self.service.inner.config = config;
            self.did_change = true;
        }
    }

    pub fn config(&self) -> &ServiceConfig {
        &self.service.inner.config
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled != self.service.inner.enabled {
            self.service.inner.enabled = enabled;
            self.did_change = true;
        }
    }

    pub fn enabled(&self) -> bool {
        self.service.inner.enabled
    }

    pub fn set_state(&mut self, state: ServiceRunState) {
        if state != self.service.inner.state {
            self.service.inner.state = state;
            self.did_change = true;
        }
    }

    pub fn state(&self) -> &ServiceRunState {
        &self.service.inner.state
    }

    pub fn set_monitor_task(&mut self, monitor_task: Option<JoinHandle<()>>) {
        self.service.monitor_task = monitor_task;
    }

    pub fn monitor_task(&self) -> Option<&JoinHandle<()>> {
        self.service.monitor_task.as_ref()
    }
}

pub struct SendableService {
    service: Service,
    did_change: bool,
    connection: ControllerConnection,
}

impl SendableService {
    pub async fn send_update_if_necessary(self) -> Result<()> {
        if !self.did_change {
            return Ok(());
        }

        match &self.connection {
            ControllerConnection::WithConnection(_) => {
                self.connection
                    .write_response(WorkerResponse::ServiceUpdate(self.service.inner))
                    .await
            }
            ControllerConnection::Disabled => {
                // Ignore
                Ok(())
            }
        }
    }
}
