use serde::{Deserialize, Serialize};

use crate::ServiceStatus;

#[derive(Serialize, Deserialize, Debug)]
pub enum RemoteCommand {
    Start(String),
    Stop(String),
    Enable(String),
    Disable(String),
    Status(String),
    List,
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum RemoteResponse {
    Success(String),
    Error(String),
    Status(ServiceStatus),
    List(Vec<ServiceStatus>),
    ShuttingDown,
}
