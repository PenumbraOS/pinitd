use serde::{Deserialize, Serialize};
use std::{
    fs::create_dir_all,
    hash::Hash,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::unit_config::ServiceConfig;

pub mod android;
pub mod bincode;
pub mod error;
pub mod protocol;
pub mod unit_config;

pub const CONTROL_SOCKET_ADDRESS: &str = "127.0.0.1:1717";
pub const WORKER_SOCKET_ADDRESS: &str = "127.0.0.1:1718";
pub const PMS_SOCKET_ADDRESS: &str = "127.0.0.1:1719";
// Bridge TCP address is 127.0.0.1:1720

#[cfg(target_os = "android")]
pub const CONFIG_DIR: &str = "/sdcard/penumbra/etc/pinitd/system/";
#[cfg(not(target_os = "android"))]
pub const CONFIG_DIR: &str = "test_data/jailbreak_units/";

#[cfg(target_os = "android")]
pub const STATE_FILE: &str = "/sdcard/penumbra/etc/pinitd/pinitd.state";
#[cfg(not(target_os = "android"))]
pub const STATE_FILE: &str = "test_data/pinitd/pinitd.state";

#[cfg(target_os = "android")]
pub const CONTROLLER_LOCK_FILE: &str = "/sdcard/penumbra/etc/pinitd/pinitd.lock";
#[cfg(not(target_os = "android"))]
pub const CONTROLLER_LOCK_FILE: &str = "test_data/pinitd/pinitd.lock";

#[cfg(target_os = "android")]
pub const ZYGOTE_READY_FILE: &str = "/sdcard/penumbra/etc/pinitd/zygote_ready";
#[cfg(not(target_os = "android"))]
pub const ZYGOTE_READY_FILE: &str = "test_data/pinitd/zygote_ready";

pub const PACKAGE_NAME: &str = "com.penumbraos.pinitd";

pub const WORKER_CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ServiceRunState {
    Stopped,
    Stopping,
    Running { pid: Option<u32> },
    Failed { reason: String },
}

impl std::fmt::Display for ServiceRunState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stopped => write!(f, "Stopped"),
            Self::Stopping => write!(f, "Stopping"),
            Self::Running { pid } => write!(
                f,
                "Running (PID: {})",
                pid.map_or("Unknown".into(), |pid| format!("{pid}"))
            ),
            Self::Failed { reason } => write!(f, "Failed: {}", reason),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServiceStatus {
    pub name: String,
    pub uid: UID,
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

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Clone)]
pub enum UID {
    System,
    Shell,
    Custom(usize),
}

impl TryFrom<&str> for UID {
    type Error = String;
    fn try_from(value: &str) -> Result<Self, String> {
        match value {
            "1000" => Ok(Self::System),
            "2000" => Ok(Self::Shell),
            _ => match value.parse::<usize>() {
                Ok(value) => Ok(Self::Custom(value)),
                Err(_) => Err(format!("Unsupported Uid \"{value}\"")),
            },
        }
    }
}

impl From<UID> for usize {
    fn from(value: UID) -> Self {
        match value {
            UID::System => 1000,
            UID::Shell => 2000,
            UID::Custom(uid) => uid,
        }
    }
}

/// Unique identifier for a worker combining UID and SE info
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkerIdentity {
    pub uid: UID,
    pub se_info: String,
}

impl WorkerIdentity {
    pub fn new(uid: UID, se_info: Option<String>) -> Self {
        let se_info = se_info.unwrap_or_else(|| Self::default_se_info(&uid));
        Self { uid, se_info }
    }

    pub fn default_se_info(uid: &UID) -> String {
        match uid {
            UID::System | UID::Custom(_) => {
                "platform:system_app:targetSdkVersion=29:complete".into()
            }
            UID::Shell => "platform:shell:targetSdkVersion=29:complete".into(),
        }
    }
}

impl From<ServiceConfig> for WorkerIdentity {
    fn from(value: ServiceConfig) -> Self {
        WorkerIdentity::new(value.command.uid, value.se_info)
    }
}
