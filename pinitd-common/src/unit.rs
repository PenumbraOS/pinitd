use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::UID;

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub enum RestartPolicy {
    Always,
    OnFailure,
    None,
}

impl TryFrom<&str> for RestartPolicy {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "always" => Ok(Self::Always),
            "on-failure" => Ok(Self::OnFailure),
            "none" => Ok(Self::None),
            _ => Err(format!("Unsupported Restart \"{value}\"")),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ServiceConfig {
    pub name: String,
    pub command: String,
    pub autostart: bool,
    pub restart: RestartPolicy,
    pub uid: UID,
    pub nice_name: Option<String>,
    pub unit_file_path: PathBuf,
}
