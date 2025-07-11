use serde::{Deserialize, Serialize};
use std::{
    fs::create_dir_all,
    path::{Path, PathBuf},
};

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
pub const BOOT_SUCCESS_FILE: &str = "/sdcard/penumbra/etc/pinitd/boot_success";
#[cfg(not(target_os = "android"))]
pub const BOOT_SUCCESS_FILE: &str = "test_data/pinitd/boot_success";

pub const PACKAGE_NAME: &str = "com.penumbraos.pinitd";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ServiceRunState {
    Stopped,
    Stopping,
    Running { pid: u32 },
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

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
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
