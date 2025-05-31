use std::sync::Arc;

use tokio::{net::tcp::OwnedWriteHalf, sync::Mutex};

pub struct ZygoteProcessConnection {
    pub stream_tx: Arc<Mutex<OwnedWriteHalf>>,
}
