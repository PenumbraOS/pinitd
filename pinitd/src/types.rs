use pinitd_common::{ServiceRunState, ServiceStatus};
use tokio::task::JoinHandle;

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
