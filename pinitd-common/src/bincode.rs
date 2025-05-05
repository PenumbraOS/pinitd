use bincode::error::{DecodeError, EncodeError};
use serde::{Serialize, de::DeserializeOwned};

pub trait Bincodable<'a>: Serialize + DeserializeOwned {
    fn encode(self) -> Result<Vec<u8>, EncodeError> {
        bincode::serde::encode_to_vec(self, bincode::config::standard())
    }

    fn decode(slice: &[u8]) -> Result<(Self, usize), DecodeError> {
        bincode::serde::decode_from_slice(slice, bincode::config::standard())
    }
}
