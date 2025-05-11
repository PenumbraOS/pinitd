use std::path::Path;

use ini::Ini;
use pinitd_common::{
    UID,
    unit::{RestartPolicy, ServiceConfig},
};
use tokio::fs;

use crate::error::{Error, Result};

pub trait ParsableServiceConfig {
    async fn parse(path: &Path) -> Result<ServiceConfig>;
}

impl ParsableServiceConfig for ServiceConfig {
    async fn parse(path: &Path) -> Result<Self> {
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
        let mut nice_name = None;
        let mut autostart = false;
        let mut restart = RestartPolicy::None;

        for (property, value) in service_section.iter() {
            match property {
                "Name" => {
                    name = Some(value.trim().to_string());
                }
                "Exec" => command = Some(value.trim().to_string()),
                "Uid" => {
                    uid = value
                        .trim()
                        .try_into()
                        .map_err(|err| Error::ConfigError(err))?
                }
                "NiceName" => {
                    nice_name = Some(value.trim().into());
                }
                "Autostart" => autostart = value.trim().eq_ignore_ascii_case("true"),
                "Restart" => {
                    restart = value
                        .trim()
                        .try_into()
                        .map_err(|err| Error::ConfigError(err))?
                }
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

        if nice_name.is_some() && uid != UID::System {
            return Err(Error::ConfigError(format!(
                "\"NiceName\" is set with a non-1000 UID. This is not currently supported"
            )));
        }

        Ok(Self {
            name,
            command,
            uid,
            nice_name,
            autostart,
            restart,
            unit_file_path: path.to_path_buf(),
        })
    }
}
