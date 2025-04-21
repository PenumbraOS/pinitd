use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
};

use bincode::{Decode, Encode};

use crate::error::Error;

#[derive(Encode, Decode)]
pub struct DataFrame {
    pub value: String,
}

impl DataFrame {
    pub fn new(value: String) -> Self {
        DataFrame { value }
    }

    pub fn receive(stream: &mut UnixStream) -> Result<Self, Error> {
        let mut len_bytes = [0u8; 4];
        stream.read_exact(&mut len_bytes)?;
        let len = u32::from_le_bytes(len_bytes);

        let mut frame_bytes = vec![0u8; len as usize];
        stream.read_exact(&mut frame_bytes)?;

        let (message, _) =
            bincode::decode_from_slice::<DataFrame, _>(&frame_bytes, bincode::config::standard())?;
        Ok(message)
    }

    pub fn send(&self, stream: &mut UnixStream) -> Result<(), Error> {
        let vec = bincode::encode_to_vec(self, bincode::config::standard())
            .expect("Failed to encode frame");

        stream.write_all(&(vec.len() as u32).to_le_bytes())?;
        stream.write_all(&vec)?;
        stream.flush()?;

        Ok(())
    }
}
