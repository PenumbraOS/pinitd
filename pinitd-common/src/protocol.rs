use serde::{Deserialize, Serialize};

use crate::{ServiceStatus, bincode::Bincodable};

#[derive(Serialize, Deserialize, Debug)]
pub enum CLICommand {
    Start(String),
    Stop(String),
    Restart(String),
    Enable(String),
    Disable(String),
    Reload(String),
    Status(String),
    List,
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum CLIResponse {
    Success(String),
    Error(String),
    Status(ServiceStatus),
    List(Vec<ServiceStatus>),
    ShuttingDown,
}

impl Bincodable<'_> for CLICommand {}
impl Bincodable<'_> for CLIResponse {}
