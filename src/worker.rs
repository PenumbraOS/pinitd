use crate::{error::Error, protocol::DataFrame, socket::open_socket};

pub struct Worker {}

impl Worker {
    pub fn create() -> Result<(), Error> {
        let listener = open_socket()?;

        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => loop {
                    match DataFrame::receive(&mut stream) {
                        Ok(frame) => {
                            warn!("Received {}", frame.value)
                        }
                        Err(error) => {
                            error!("Could not decode frame: {error}");
                            break;
                        }
                    }
                },
                Err(error) => error!("Error opening socket stream: {error}"),
            }
        }

        Ok(())
    }
}
