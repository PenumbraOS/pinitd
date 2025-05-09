use std::path::{Path, PathBuf};

use ini::Ini;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::error::{Error, Result};

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub enum RestartPolicy {
    Always,
    OnFailure,
    None,
}

impl TryFrom<&str> for RestartPolicy {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "always" => Ok(Self::Always),
            "on-failure" => Ok(Self::OnFailure),
            "none" => Ok(Self::None),
            _ => Err(Error::ConfigError(format!(
                "Unsupported Restart \"{value}\""
            ))),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub enum UID {
    System = 1000,
    Shell = 2000,
}

impl TryFrom<&str> for UID {
    type Error = Error;
    fn try_from(value: &str) -> Result<Self> {
        match value {
            "1000" => Ok(Self::System),
            "2000" => Ok(Self::Shell),
            _ => Err(Error::ConfigError(format!("Unsupported Uid \"{value}\""))),
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
    pub async fn parse(path: &Path) -> Result<Self> {
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

        let mut name = None;
        let mut command = None;
        let mut uid = UID::Shell;
        let mut autostart = false;
        let mut restart = RestartPolicy::None;

        for (property, value) in service_section.iter() {
            match property {
                "Name" => {
                    name = Some(value.trim().to_string());
                }
                "Exec" => command = Some(value.trim().to_string()),
                "Uid" => uid = value.trim().try_into()?,
                "Autostart" => autostart = value.trim().eq_ignore_ascii_case("true"),
                "Restart" => restart = value.trim().try_into()?,
                _ => {
                    return Err(Error::ConfigError(format!(
                        "Unsupported property \"{property}\""
                    )));
                }
            }
        }

        let name = if let Some(name) = name {
            if name.is_empty() {
                return Err(Error::ConfigError("\"Name\" cannot be empty".into()));
            }

            name
        } else {
            return Err(Error::ConfigError("\"Name\" must be provided".into()));
        };

        let command = if let Some(command) = command {
            if command.is_empty() {
                return Err(Error::ConfigError("\"Exec\" cannot be empty".into()));
            }

            command
        } else {
            return Err(Error::ConfigError("\"Exec\" must be provided".into()));
        };

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
