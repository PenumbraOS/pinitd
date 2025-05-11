use serde::{Deserialize, Serialize};

use crate::{ServiceStatus, bincode::Bincodable, unit::ServiceConfig};

#[derive(Serialize, Deserialize, Debug)]
pub enum CLICommand {
    Start(String),
    Stop(String),
    Restart(String),
    Enable(String),
    Disable(String),
    Reload(String),
    ReloadAll,
    Status(String),
    Config(String),
    List,
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum CLIResponse {
    Success(String),
    Error(String),
    Status(ServiceStatus),
    List(Vec<ServiceStatus>),
    Config(ServiceConfig),
    ShuttingDown,
}

impl Bincodable<'_> for CLICommand {}
impl Bincodable<'_> for CLIResponse {}
