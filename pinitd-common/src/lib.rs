use serde::{Deserialize, Serialize};
use std::{
    fs::create_dir_all,
    path::{Path, PathBuf},
};

pub mod bincode;
pub mod protocol;

pub const SOCKET_ADDRESS: &str = "127.0.0.1:1717";

#[cfg(target_os = "android")]
pub const CONFIG_DIR: &str = "/data/local/tmp/jailbreak_units/";
#[cfg(not(target_os = "android"))]
pub const CONFIG_DIR: &str = "test_data/jailbreak_units/";

#[cfg(target_os = "android")]
pub const STATE_FILE: &str = "/data/local/tmp/pinitd/initd.state";
#[cfg(not(target_os = "android"))]
pub const STATE_FILE: &str = "test_data/pinitd/initd.state";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ServiceRunState {
    Stopped,
    Stopping,
    Running { pid: i32 },
    Failed { reason: String },
}

impl std::fmt::Display for ServiceRunState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stopped => write!(f, "Stopped"),
            Self::Stopping => write!(f, "Stopping"),
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

pub fn create_core_directories() {
    let _ = create_dir_all(CONFIG_DIR);

    if let Some(parent) = Path::new(STATE_FILE).parent() {
        let _ = create_dir_all(parent);
    }
}
