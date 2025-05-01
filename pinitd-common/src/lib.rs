use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// --- Constants ---
// Note: These paths assume execution as root or appropriate permissions.
// Adjust if running in a different context.
pub const SOCKET_PATH: &str = "/data/local/tmp/jailbreak/pinitd/initd.sock";
pub const CONFIG_DIR: &str = "/data/local/jailbreak_units";
pub const STATE_FILE: &str = "/data/local/tmp/initd.state";

// --- IPC Command/Response ---

#[derive(Serialize, Deserialize, Debug)]
pub enum Command {
    Start(String),
    Stop(String),
    Enable(String),
    Disable(String),
    Status(String),
    List,
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ServiceRunState {
    Stopped,
    Running { pid: u32 },
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

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Success(String),
    Error(String),
    Status(ServiceStatus),
    List(Vec<ServiceStatus>),
    ShuttingDown,
}
