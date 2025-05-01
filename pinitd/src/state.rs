use pinitd_common::STATE_FILE;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::error::Error;

#[derive(Serialize, Deserialize)]
pub struct State {
    pub enabled_services: Vec<String>,
}

impl State {
    pub async fn load() -> Result<Self, Error> {
        match fs::read_to_string(STATE_FILE).await {
            Ok(content) => Ok(serde_json::from_str(&content)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                info!(
                    "State file {} not found, assuming no services are enabled.",
                    STATE_FILE
                );
                Ok(State {
                    enabled_services: Vec::new(),
                })
            }
            Err(e) => Err(e)?,
        }
    }
}
