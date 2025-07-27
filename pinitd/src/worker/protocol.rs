use std::collections::HashMap;

use pinitd_common::{
    ServiceRunState, UID,
    bincode::Bincodable,
    protocol::writable::{ProtocolRead, ProtocolWrite},
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

/// Unified message type for worker→controller communication
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub enum WorkerMessage {
    /// Response to a specific command
    Response(WorkerResponse),
    /// Proactive event from worker
    Event(WorkerEvent),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum WorkerCommand {
    /// Spawn a process directly with the given command
    SpawnProcess {
        command: String,
        pinit_id: Uuid,
        service_name: String,
    },
    /// Kill a process by service name
    KillProcess { service_name: String },
    /// Get status of all running processes (on-demand)
    Status,
    /// Shutdown worker
    Shutdown,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub enum WorkerResponse {
    /// Command was processed successfully
    Success,
    /// Error occurred
    Error(String),
    /// Status of all running processes (response to Status command)
    Status(HashMap<String, ServiceRunState>),
    /// Worker is shutting down
    ShuttingDown,
}

/// Events that workers push to controller proactively
#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub enum WorkerEvent {
    WorkerRegistration {
        worker_uid: UID,
    },
    Heartbeat {
        worker_uid: UID,
        uptime_seconds: u64,
        active_services: u32,
    },
    ProcessSpawned {
        service_name: String,
        pinit_id: Uuid,
    },
    ProcessExited {
        service_name: String,
        exit_code: i32,
    },
    ProcessCrashed {
        service_name: String,
        signal: i32,
    },
    WorkerError {
        service_name: Option<String>,
        error: String,
    },
}

impl Bincodable<'_> for WorkerCommand {}
impl Bincodable<'_> for WorkerResponse {}
impl Bincodable<'_> for WorkerEvent {}
impl Bincodable<'_> for WorkerMessage {}

impl<T> ProtocolRead<'_, T> for WorkerCommand where T: AsyncReadExt + Unpin + Send {}
impl<T> ProtocolRead<'_, T> for WorkerResponse where T: AsyncReadExt + Unpin + Send {}
impl<T> ProtocolRead<'_, T> for WorkerEvent where T: AsyncReadExt + Unpin + Send {}
impl<T> ProtocolRead<'_, T> for WorkerMessage where T: AsyncReadExt + Unpin + Send {}

impl<T> ProtocolWrite<'_, T> for WorkerCommand where T: AsyncWriteExt + Unpin + Send {}
impl<T> ProtocolWrite<'_, T> for WorkerResponse where T: AsyncWriteExt + Unpin + Send {}
impl<T> ProtocolWrite<'_, T> for WorkerEvent where T: AsyncWriteExt + Unpin + Send {}
impl<T> ProtocolWrite<'_, T> for WorkerMessage where T: AsyncWriteExt + Unpin + Send {}
