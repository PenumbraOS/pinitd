use bincode::error::{DecodeError, EncodeError};
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

impl RemoteCommand {
    pub fn encode(self) -> Result<Vec<u8>, EncodeError> {
        bincode::serde::encode_to_vec(self, bincode::config::standard())
    }

    pub fn decode(slice: &[u8]) -> Result<(Self, usize), DecodeError> {
        bincode::serde::decode_from_slice(slice, bincode::config::standard())
    }
}

impl RemoteResponse {
    pub fn encode(self) -> Result<Vec<u8>, EncodeError> {
        bincode::serde::encode_to_vec(self, bincode::config::standard())
    }

    pub fn decode(slice: &[u8]) -> Result<(Self, usize), DecodeError> {
        bincode::serde::decode_from_slice(slice, bincode::config::standard())
    }
}
