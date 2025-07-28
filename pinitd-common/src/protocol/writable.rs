use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{bincode::Bincodable, error::Result};

type LengthType = u64;

pub trait ProtocolRead<'a, S>
where
    Self: Bincodable<'a>,
    S: AsyncReadExt + Unpin + Send,
{
    fn read(stream: &mut S) -> impl std::future::Future<Output = Result<Self>> + Send {
        async {
            match read_internal(stream).await {
                Ok(result) => Ok(result),
                Err(err) => Err(err),
            }
        }
    }
}

// This is kept outside of the ProtocolRead body as there are weird Rust restrictions to async in trait
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

pub trait ProtocolWrite<'a, S>
where
    Self: Bincodable<'a> + Send,
    S: AsyncWriteExt + Unpin + Send,
{
    fn write(self, stream: &mut S) -> impl std::future::Future<Output = Result<()>> + Send {
        async {
            let buffer = self.encode()?;

            let len_bytes = (buffer.len() as LengthType).to_le_bytes();
            stream.write_all(&len_bytes).await?;
            stream.write_all(&buffer).await?;
            stream.flush().await?;

            Ok(())
        }
    }
}
