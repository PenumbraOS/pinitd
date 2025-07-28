use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;
use writable::{ProtocolRead, ProtocolWrite};

use crate::{ServiceStatus, bincode::Bincodable, unit_config::ServiceConfig};

pub mod writable;

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
    // SpawnAppProcess {
    //     jvm_path: String,
    //     uid: UID,
    //     se_info: String,
    // },
    List,
    Shutdown,
    /// Hidden command used to signal Zygote has been restarted after reparenting cgroups. See #4 for more information
    ZygoteReady,
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

impl<T> ProtocolRead<'_, T> for CLICommand where T: AsyncReadExt + Unpin + Send {}
impl<T> ProtocolRead<'_, T> for CLIResponse where T: AsyncReadExt + Unpin + Send {}

impl<T> ProtocolWrite<'_, T> for CLICommand where T: AsyncWriteExt + Unpin + Send {}
impl<T> ProtocolWrite<'_, T> for CLIResponse where T: AsyncWriteExt + Unpin + Send {}

#[derive(Serialize, Deserialize, Debug)]
pub enum PMSFromRemoteCommand {
    /// Zygote spawn command ID
    WrapperLaunched(Uuid),
    /// Actual process pid
    ProcessAttached(u32),
    /// Process exit code
    ProcessExited(Option<i32>),
}

// #[derive(Serialize, Deserialize, Debug)]
// pub enum PMSFromRemoteResponse {
//     /// Process is expected, stay alive
//     AllowProcess,
//     KillProcess
// }

#[derive(Serialize, Deserialize, Debug)]
pub enum PMSToRemoteCommand {
    AllowStart,
    Kill,
    Ack,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum PMSToRemoteResponse {
    Ack,
}

impl Bincodable<'_> for PMSFromRemoteCommand {}
// impl Bincodable<'_> for PMSFromRemoteResponse {}
impl Bincodable<'_> for PMSToRemoteCommand {}
impl Bincodable<'_> for PMSToRemoteResponse {}

impl<T> ProtocolRead<'_, T> for PMSFromRemoteCommand where T: AsyncReadExt + Unpin + Send {}
impl<T> ProtocolRead<'_, T> for PMSToRemoteCommand where T: AsyncReadExt + Unpin + Send {}
impl<T> ProtocolRead<'_, T> for PMSToRemoteResponse where T: AsyncReadExt + Unpin + Send {}

impl<T> ProtocolWrite<'_, T> for PMSFromRemoteCommand where T: AsyncWriteExt + Unpin + Send {}
impl<T> ProtocolWrite<'_, T> for PMSToRemoteCommand where T: AsyncWriteExt + Unpin + Send {}
impl<T> ProtocolWrite<'_, T> for PMSToRemoteResponse where T: AsyncWriteExt + Unpin + Send {}
