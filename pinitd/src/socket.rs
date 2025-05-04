use std::io::ErrorKind;

use pinitd_common::SOCKET_PATH;
use tokio::fs;
use tokio::net::UnixListener;

use crate::error::Error;

pub async fn register_socket() -> Result<UnixListener, Error> {
    info!("Registering socket");
    if fs::metadata(SOCKET_PATH).await.is_ok() {
        info!("Removing existing socket file: {}", SOCKET_PATH);
        fs::remove_file(SOCKET_PATH).await?;
    }

    let listener = UnixListener::bind(SOCKET_PATH)?;
    info!("Listening for commands on {}", SOCKET_PATH);

    Ok(listener)
}

pub async fn close_socket() {
    info!("Cleaning up socket file: {}", SOCKET_PATH);
    if let Err(e) = fs::remove_file(SOCKET_PATH).await {
        if e.kind() != ErrorKind::NotFound {
            error!("Failed to remove socket file during shutdown: {}", e);
        }
    }
}
