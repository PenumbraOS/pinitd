use std::collections::HashMap;

use pinitd_common::{
    ServiceRunState,
    bincode::Bincodable,
    protocol::writable::{ProtocolRead, ProtocolWrite},
    unit_config::ServiceConfig,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

use crate::types::BaseService;

#[derive(Serialize, Deserialize, Debug)]
pub enum WorkerCommand {
    /// Create or replace/update
    Create(ServiceConfig),
    Destroy(String),
    Start {
        service_name: String,
        pinit_id: Uuid,
    },
    Stop(String),
    Restart(String),
    Status,
    Shutdown,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub enum WorkerResponse {
    Success,
    Error(String),
    Status(HashMap<String, ServiceRunState>),
    ServiceUpdate(BaseService),
    ShuttingDown,
}

impl Bincodable<'_> for WorkerCommand {}
impl Bincodable<'_> for WorkerResponse {}

impl<T> ProtocolRead<'_, T> for WorkerCommand where T: AsyncReadExt + Unpin + Send {}
impl<T> ProtocolRead<'_, T> for WorkerResponse where T: AsyncReadExt + Unpin + Send {}

impl<T> ProtocolWrite<'_, T> for WorkerCommand where T: AsyncWriteExt + Unpin + Send {}
impl<T> ProtocolWrite<'_, T> for WorkerResponse where T: AsyncWriteExt + Unpin + Send {}
