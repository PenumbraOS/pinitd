use pinitd_common::{ServiceRunState, ServiceStatus, unit_config::ServiceConfig};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct BaseService {
    pub config: ServiceConfig,
    pub state: ServiceRunState,
    pub enabled: bool,
}

pub struct Service {
    inner: BaseService,
}

impl Clone for Service {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
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

    pub fn set_state(&mut self, state: ServiceRunState) {
        self.inner.state = state;
    }
}
