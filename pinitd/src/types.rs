use pinitd_common::{ServiceRunState, ServiceStatus};
use std::{collections::HashMap, sync::Arc};
use tokio::{sync::Mutex, task::JoinHandle};

use crate::unit::ServiceConfig;

#[derive(Debug)]
pub struct Service {
    pub config: ServiceConfig,
    pub state: ServiceRunState,
    pub enabled: bool,
    pub monitor_task: Option<JoinHandle<()>>,
}

impl Service {
    pub fn status(&self) -> ServiceStatus {
        ServiceStatus {
            name: self.config.name.clone(),
            enabled: self.enabled,
            state: self.state.clone(),
            config_path: self.config.unit_file_path.clone(),
        }
    }
}

pub type ServiceRegistry = Arc<Mutex<HashMap<String, Service>>>;
