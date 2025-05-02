use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub mod protocol;

pub const SOCKET_PATH: &str = "/data/local/tmp/jailbreak/pinitd/initd.sock";
pub const CONFIG_DIR: &str = "/data/local/jailbreak_units";
pub const STATE_FILE: &str = "/data/local/tmp/initd.state";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ServiceRunState {
    Stopped,
    Running { pid: i32 },
    Failed { reason: String },
}

impl std::fmt::Display for ServiceRunState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stopped => write!(f, "Stopped"),
            Self::Running { pid } => write!(f, "Running (PID: {})", pid),
            Self::Failed { reason } => write!(f, "Failed: {}", reason),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServiceStatus {
    pub name: String,
    pub enabled: bool,
    pub state: ServiceRunState,
    pub config_path: PathBuf,
}
