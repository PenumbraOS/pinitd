use std::sync::Arc;

use tokio::{net::tcp::OwnedWriteHalf, sync::Mutex};
use uuid::Uuid;

#[derive(Clone)]
pub struct ZygoteProcessConnection {
    pub pinit_id: Uuid,
    pub service_name: String,
    #[allow(dead_code)]
    pub stream_tx: Arc<Mutex<OwnedWriteHalf>>,
}
