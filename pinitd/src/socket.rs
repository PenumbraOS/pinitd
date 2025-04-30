use std::io;
use std::os::android::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};

const WORKER_SOCKET_NAME: &str = "\0pinit-worker";

pub fn connect_worker() -> Result<UnixStream, io::Error> {
    warn!("Attempting to connect to worker");
    let address = address()?;
    UnixStream::connect_addr(&address)
}

pub fn open_socket() -> Result<UnixListener, io::Error> {
    warn!("Attempting to open socket");
    let address = address()?;
    UnixListener::bind_addr(&address)
}

fn address() -> Result<SocketAddr, io::Error> {
    match SocketAddr::from_abstract_name(WORKER_SOCKET_NAME) {
        Ok(address) => Ok(address),
        Err(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid socket name",
        )),
    }
}
