use pinitd_common::bincode::Bincodable;
use serde::{Deserialize, Serialize};

use crate::unit::ServiceConfig;

pub const WORKER_COMMAND_LENGTH_COUNT: usize = 8;

#[derive(Serialize, Deserialize, Debug)]
pub enum WorkerCommand {
    Create(ServiceConfig),
    Destroy(String),
    Start(String),
    Stop(String),
    Restart(String),
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum WorkerResponse {
    Success,
    Error(String),
    ShuttingDown,
}

impl Bincodable<'_> for WorkerCommand {}
impl Bincodable<'_> for WorkerResponse {}
