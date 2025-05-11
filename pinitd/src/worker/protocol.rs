use std::collections::HashMap;

use pinitd_common::{ServiceRunState, bincode::Bincodable, unit::ServiceConfig};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{
    error::{Error, Result},
    types::BaseService,
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
    ServiceUpdate(BaseService),
    ShuttingDown,
}

impl Bincodable<'_> for WorkerCommand {}
impl Bincodable<'_> for WorkerResponse {}

pub trait WorkerRead<'a, S>
where
    Self: Bincodable<'a>,
    S: AsyncReadExt + Unpin,
{
    async fn read(stream: &mut S) -> Result<Self> {
        match read_internal(stream).await {
            Ok(result) => Ok(result),
            Err(err) => Err(err),
        }
    }
}

type LengthType = u64;

async fn read_internal<'a, S, T>(stream: &mut S) -> Result<T>
where
    T: Bincodable<'a>,
    S: AsyncReadExt + Unpin,
{
    let mut len_bytes = [0; std::mem::size_of::<LengthType>()];

    stream.read_exact(&mut len_bytes).await?;
    let len = LengthType::from_le_bytes(len_bytes);

    let mut buffer = vec![0; len as usize];
    stream.read_exact(&mut buffer).await?;

    let (result, _) = T::decode(&buffer)?;
    Ok(result)
}

pub trait WorkerWrite<'a, S>
where
    Self: Bincodable<'a>,
    S: AsyncWriteExt + Unpin,
{
    async fn write(self, stream: &mut S) -> Result<()> {
        let buffer = self.encode()?;

        let len_bytes = (buffer.len() as LengthType).to_le_bytes();
        stream.write_all(&len_bytes).await?;
        stream.write_all(&buffer).await?;

        Ok(())
    }
}

impl<T> WorkerRead<'_, T> for WorkerCommand where T: AsyncReadExt + Unpin {}
impl<T> WorkerRead<'_, T> for WorkerResponse where T: AsyncReadExt + Unpin {}

impl<T> WorkerWrite<'_, T> for WorkerCommand where T: AsyncWriteExt + Unpin {}
impl<T> WorkerWrite<'_, T> for WorkerResponse where T: AsyncWriteExt + Unpin {}
