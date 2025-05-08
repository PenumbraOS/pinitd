use std::collections::HashMap;

use pinitd_common::{ServiceRunState, bincode::Bincodable};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{
    error::{Error, Result},
    types::BaseService,
    unit::ServiceConfig,
};

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

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub enum WorkerResponse {
    Success,
    Error(String),
    Status(HashMap<String, ServiceRunState>),
    ShuttingDown,
}

pub type WorkerServiceUpdate = BaseService;

impl Bincodable<'_> for WorkerCommand {}
impl Bincodable<'_> for WorkerResponse {}
impl Bincodable<'_> for WorkerServiceUpdate {}

pub trait WorkerRead<'a, S>
where
    Self: Bincodable<'a>,
    S: AsyncReadExt + Unpin,
{
    async fn read(stream: &mut S) -> Result<Self> {
        let mut len_bytes = [0; std::mem::size_of::<u64>()];

        match stream.read_exact(&mut len_bytes).await {
            Ok(_) => {
                stream.read_exact(&mut len_bytes).await?;
                let len = u64::from_le_bytes(len_bytes);

                let mut buffer = vec![0; len as usize];
                stream.read_exact(&mut buffer).await?;

                let (result, _) = Self::decode(&buffer)?;
                Ok(result)
            }
            Err(err) => Err(Error::WorkerProtocolError(err.to_string())),
        }
    }
}

pub trait WorkerWrite<'a, S>
where
    Self: Bincodable<'a>,
    S: AsyncWriteExt + Unpin,
{
    async fn write(self, stream: &mut S) -> Result<()> {
        let buffer = self.encode()?;

        let len_bytes = (buffer.len() as u64).to_le_bytes();
        stream.write_all(&len_bytes).await?;
        stream.write_all(&buffer).await?;

        Ok(())
    }
}

impl<T> WorkerRead<'_, T> for WorkerCommand where T: AsyncReadExt + Unpin {}
impl<T> WorkerRead<'_, T> for WorkerResponse where T: AsyncReadExt + Unpin {}
impl<T> WorkerRead<'_, T> for WorkerServiceUpdate where T: AsyncReadExt + Unpin {}

impl<T> WorkerWrite<'_, T> for WorkerCommand where T: AsyncWriteExt + Unpin {}
impl<T> WorkerWrite<'_, T> for WorkerResponse where T: AsyncWriteExt + Unpin {}
impl<T> WorkerWrite<'_, T> for WorkerServiceUpdate where T: AsyncWriteExt + Unpin {}
