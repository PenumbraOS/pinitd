use serde::{Deserialize, Serialize};
use std::{
    fs::create_dir_all,
    path::{Path, PathBuf},
};

pub mod protocol;

#[cfg(target_os = "android")]
pub const SOCKET_PATH: &str = "/data/local/tmp/jailbreak/pinitd/initd.sock";
#[cfg(not(target_os = "android"))]
pub const SOCKET_PATH: &str = "test_data/pinitd/initd.sock";

#[cfg(target_os = "android")]
pub const CONFIG_DIR: &str = "/data/local/jailbreak_units/";
#[cfg(not(target_os = "android"))]
pub const CONFIG_DIR: &str = "test_data/jailbreak_units/";

#[cfg(target_os = "android")]
pub const STATE_FILE: &str = "/data/local/tmp/pinitd/initd.state";
#[cfg(not(target_os = "android"))]
pub const STATE_FILE: &str = "test_data/pinitd/initd.state";

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

pub fn create_core_directories() {
    if let Some(parent) = Path::new(SOCKET_PATH).parent() {
        let _ = create_dir_all(parent);
    }

    if let Some(parent) = Path::new(CONFIG_DIR).parent() {
        let _ = create_dir_all(parent);
    }

    if let Some(parent) = Path::new(STATE_FILE).parent() {
        let _ = create_dir_all(parent);
    }
}
