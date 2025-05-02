use pinitd_common::STATE_FILE;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::error::Error;

#[derive(Clone, Serialize, Deserialize)]
pub struct StoredState {
    pub enabled_services: Vec<String>,
}

impl StoredState {
    pub async fn load() -> Result<Self, Error> {
        match fs::read_to_string(STATE_FILE).await {
            Ok(content) => Ok(serde_json::from_str(&content)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                info!(
                    "State file {} not found, assuming no services are enabled.",
                    STATE_FILE
                );
                Ok(Self {
                    enabled_services: Vec::new(),
                })
            }
            Err(e) => Err(e)?,
        }
    }

    pub async fn save(self) -> Result<(), Error> {
        let content = serde_json::to_string_pretty(&self)?;

        fs::write(STATE_FILE, content).await?;
        info!("Wrote updated state");

        Ok(())
    }
}
