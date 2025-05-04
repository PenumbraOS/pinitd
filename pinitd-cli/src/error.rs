use std::io;

use bincode::error::{DecodeError, EncodeError};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error {0}")]
    IO(#[from] io::Error),
    #[error("Bincode encode error {0}")]
    Encode(#[from] EncodeError),
    #[error("Bincode decode error {0}")]
    Decode(#[from] DecodeError),
    #[error("Arg parse error {0}")]
    ArgError(#[from] clap::Error),

    #[error("Unknown error {0}")]
    Unknown(String),
}
