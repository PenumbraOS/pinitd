use pinitd_common::STATE_FILE;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::error::{Error, Result};

#[derive(Clone, Serialize, Deserialize)]
pub struct StoredState {
    pub enabled_services: Vec<String>,
    is_dummy: bool,
}

impl StoredState {
    /// Variant of StoredState that always marks everything as enabled
    pub fn dummy() -> Self {
        Self {
            enabled_services: Vec::new(),
            is_dummy: true,
        }
    }

    pub async fn load() -> Result<Self> {
        match fs::read_to_string(STATE_FILE).await {
            Ok(content) => Ok(serde_json::from_str(&content)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                info!(
                    "State file {} not found, assuming no services are enabled.",
                    STATE_FILE
                );
                Ok(Self {
                    enabled_services: Vec::new(),
                    is_dummy: false,
                })
            }
            Err(e) => Err(Error::ConfigError(e.to_string()))?,
        }
    }

    pub async fn save(self) -> Result<()> {
        if self.is_dummy {
            return Ok(());
        }

        let content = serde_json::to_string_pretty(&self)?;

        fs::write(STATE_FILE, content).await?;
        info!("Wrote updated state");

        Ok(())
    }

    pub async fn enable_service(&mut self, name: String) {
        if self.is_dummy {
            return;
        }

        if self.enabled_services.iter().find(|s| **s == name).is_some() {
            // Service is not already enabled
            self.enabled_services.push(name);
            // Since it doesn't matter clone the state before saving for nicer async
            self.clone().save().await;
        }
    }

    pub async fn disable_service(&mut self, name: String) {
        if self.is_dummy {
            return;
        }

        if let Some(i) = self.enabled_services.iter().position(|s| *s == name) {
            self.enabled_services.swap_remove(i);
            // Since it doesn't matter clone the state before saving for nicer async
            self.clone().save().await;
        }
    }

    pub fn enabled(&self, name: &String) -> bool {
        self.is_dummy || self.enabled_services.contains(name)
    }
}
