use std::collections::HashMap;

use pinitd_common::{ServiceRunState, bincode::Bincodable};
use serde::{Deserialize, Serialize};

use crate::unit::ServiceConfig;

pub const WORKER_COMMAND_LENGTH_COUNT: usize = 8;

#[derive(Serialize, Deserialize, Debug)]
pub enum WorkerCommand {
    /// Create or replace/update
    Create(ServiceConfig),
    Destroy(String),
    Start(String),
    Stop(String),
    Restart(String),
    Status,
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum WorkerResponse {
    Success,
    Error(String),
    Status(HashMap<String, ServiceRunState>),
    ShuttingDown,
}

impl Bincodable<'_> for WorkerCommand {}
impl Bincodable<'_> for WorkerResponse {}
