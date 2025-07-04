use std::{io, num::ParseIntError};

use bincode::error::{DecodeError, EncodeError};
use thiserror::Error;
use tokio::{sync::broadcast, time::error::Elapsed};

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error {0}")]
    IO(#[from] io::Error),
    #[error("Bincode encode error {0}")]
    Encode(#[from] EncodeError),
    #[error("Bincode decode error {0}")]
    Decode(#[from] DecodeError),
    #[error("Fd parse error {0}")]
    ParseIntError(#[from] ParseIntError),
    #[error("Arg parse error {0}")]
    ArgError(#[from] clap::Error),

    #[error("Error reading from worker bridge: {0}")]
    WorkerProtocolError(String),
    #[error("Worker timeout error: {0}")]
    WorkerTimeoutError(#[from] Elapsed),
    #[error("Connection error {0}")]
    WorkerConnectionRecvError(#[from] broadcast::error::RecvError),

    #[error("Unknown service: \"{0}\"")]
    UnknownServiceError(String),
    #[error("Failed to parse config: {0}")]
    ConfigError(String),
    #[error("Failed to parse persistent state: {0}")]
    ParseStateError(#[from] serde_json::Error),
    #[error("Error spawning process: {0}")]
    ProcessSpawnError(String),
    #[error("Exploit error: {0}")]
    ExploitError(#[from] android_31317_exploit::error::Error),
    #[error("Common error: {0}")]
    CommonError(#[from] pinitd_common::error::Error),
    #[error("Zygote error: {0}")]
    ZygoteError(String),
    #[error("Unknown error: {0}")]
    Unknown(String),
}

pub type Result<T> = std::result::Result<T, Error>;
