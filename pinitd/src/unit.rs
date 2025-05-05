use std::path::{Path, PathBuf};

use ini::Ini;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::error::Error;

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub enum RestartPolicy {
    Always,
    OnFailure,
    None,
}

impl From<&str> for RestartPolicy {
    fn from(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "always" => Self::Always,
            "on-failure" => Self::OnFailure,
            _ => Self::None,
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub enum UID {
    System = 1000,
    Shell = 2000,
}

impl From<&str> for UID {
    fn from(value: &str) -> Self {
        match value {
            "1000" => Self::System,
            "2000" | _ => Self::Shell,
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
    pub unit_file_path: PathBuf,
}

impl ServiceConfig {
    pub async fn parse(path: &Path) -> Result<Self, Error> {
        let content = fs::read_to_string(path).await.or_else(|_| {
            Err(Error::Unknown(format!(
                "Failed to read unit file {:?}",
                path
            )))
        })?;
        let ini = Ini::load_from_str(&content)
            .map_err(|e| Error::ConfigError(format!("INI parsing error: {e}")))?;

        let service_section = ini
            .section(Some("Service"))
            .ok_or_else(|| Error::ConfigError("Missing [Service] section".into()))?;

        let name = service_section
            .get("Name")
            .ok_or_else(|| Error::ConfigError("Missing \"Name\" key in [Service]".into()))?
            .trim()
            .to_string();

        let command = service_section
            .get("Exec")
            .ok_or_else(|| Error::ConfigError("Missing \"Exec\" key in [Service]".into()))?
            .trim()
            .to_string();

        let uid = service_section
            .get("Uid")
            .map_or(UID::Shell, |uid| uid.into());

        let autostart = service_section
            .get("Autostart")
            .map_or(false, |v| v.trim().eq_ignore_ascii_case("true"));

        let restart = service_section
            .get("Restart")
            .map_or(RestartPolicy::None, |r| r.into());

        if name.is_empty() || command.is_empty() {
            return Err(Error::ConfigError(
                "\"Name\" and \"Exec\" cannot be empty".into(),
            ));
        }

        Ok(Self {
            name,
            command,
            uid,
            autostart,
            restart,
            unit_file_path: path.to_path_buf(),
        })
    }
}
