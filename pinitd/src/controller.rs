use std::{io::ErrorKind, path::PathBuf, thread::sleep, time::Duration};

use android_31317_exploit::exploit::{ExploitKind, TriggerApp, build_and_execute};

use crate::{error::Error, protocol::DataFrame, socket::connect_worker};

pub struct Controller {}

impl Controller {
    pub fn create() -> Result<(), Error> {
        // let mut worker_socket = match connect_worker() {
        //     Ok(socket) => Ok(socket),
        //     Err(error) => match error.kind() {
        //         ErrorKind::ConnectionRefused => {
        //             // Worker isn't alive, attempt to start
        //             warn!("Worker doesn't appear to be alive. Waiting 2s");
        //             sleep(Duration::from_secs(2));
        //             warn!("Attempting to start worker");
        //         }
        //         _ => Err(error),
        //     },
        // }?;

        // worker_socket.set_read_timeout(None)?;

        // DataFrame::new("Hello world".into()).send(&mut worker_socket)?;

        // worker_socket.shutdown(std::net::Shutdown::Both)?;
        warn!("Controller started");

        Ok(())
    }
}
