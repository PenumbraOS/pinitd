use std::path::Path;

use ini::Ini;
use pinitd_common::{
    UID,
    unit_config::{RestartPolicy, ServiceCommand, ServiceConfig},
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
        let mut se_info = None;
        let mut nice_name = None;
        let mut autostart = false;
        let mut restart = RestartPolicy::None;

        for (property, value) in service_section.iter() {
            match property {
                "Name" => {
                    name = Some(value.trim().to_string());
                }
                "Exec" => command = Some(ServiceCommand::Command(value.trim().to_string())),
                "ExecPackage" => {
                    let mut iter = value.trim().splitn(2, "/");
                    let package = iter.next();
                    let content_path = iter.next();

                    if package.is_none() {
                        return Err(Error::ConfigError(
                            "Could not parse ExecPackage: No package".into(),
                        ));
                    }

                    if content_path.is_none() {
                        return Err(Error::ConfigError(
                            "Could not parse ExecPackage: No content path".into(),
                        ));
                    }

                    command = Some(ServiceCommand::LaunchPackage {
                        package: package.unwrap().to_string(),
                        content_path: content_path.unwrap().to_string(),
                    });
                }
                "ExecJvmClass" => {
                    let mut iter = value.trim().splitn(2, "/");
                    let package = iter.next();
                    let class = iter.next();

                    if package.is_none() {
                        return Err(Error::ConfigError(
                            "Could not parse ExecJvmClass: No package".into(),
                        ));
                    }

                    if class.is_none() {
                        return Err(Error::ConfigError(
                            "Could not parse ExecJvmClass: No class".into(),
                        ));
                    }

                    command = Some(ServiceCommand::JVMClass {
                        package: package.unwrap().to_string(),
                        class: class.unwrap().to_string(),
                    });
                }
                "Uid" => {
                    uid = value
                        .trim()
                        .try_into()
                        .map_err(|err| Error::ConfigError(err))?
                }
                "SeInfo" => {
                    se_info = Some(value.trim().into());
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
            match command {
                ServiceCommand::Command(ref command_string) => {
                    if command_string.is_empty() {
                        return Err(Error::ConfigError("\"Exec\" cannot be empty".into()));
                    }
                }
                ServiceCommand::LaunchPackage {
                    ref package,
                    ref content_path,
                } => {
                    if package.is_empty() {
                        return Err(Error::ConfigError(
                            "\"ExecPackage\" must contain a package".into(),
                        ));
                    }

                    if content_path.is_empty() {
                        return Err(Error::ConfigError(
                            "\"ExecPackage\" must contain a content path".into(),
                        ));
                    }
                }
                ServiceCommand::JVMClass {
                    ref package,
                    ref class,
                } => {
                    if package.is_empty() {
                        return Err(Error::ConfigError(
                            "\"ExecJVMClass\" must contain a package".into(),
                        ));
                    }

                    if class.is_empty() {
                        return Err(Error::ConfigError(
                            "\"ExecJVMClass\" must contain a class".into(),
                        ));
                    }
                }
            }

            command
        } else {
            return Err(Error::ConfigError(
                "\"Exec\" or \"ExecPackage\" must be provided".into(),
            ));
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
            se_info,
            nice_name,
            autostart,
            restart,
            unit_file_path: path.to_path_buf(),
        })
    }
}
