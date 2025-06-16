use std::path::Path;

use ini::Ini;
use pinitd_common::{
    UID,
    unit_config::{
        ExploitTriggerActivity, RestartPolicy, ServiceCommand, ServiceConfig, ServiceDependencies,
    },
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
        let mut extra_command_args = None;
        let mut extra_jvm_args = None;
        let mut trigger_app = None;
        let mut uid = UID::Shell;
        let mut se_info = None;
        let mut nice_name = None;
        let mut autostart = false;
        let mut restart = RestartPolicy::None;

        let mut dependencies = ServiceDependencies::default();
        if let Some(unit_section) = ini.section(Some("Unit")) {
            for (property, value) in unit_section.iter() {
                if property == "Wants" {
                    dependencies.wants = value.split(',').map(|s| s.trim().to_string()).collect();
                }
            }
        }

        for (property, value) in service_section.iter() {
            match property {
                "Name" => {
                    name = Some(value.trim().to_string());
                }
                "Exec" => {
                    command = Some(ServiceCommand::Command {
                        command: value.trim().to_string(),
                        trigger_activity: None,
                    })
                }
                "ExecActivity" => {
                    let (package, activity) = extract_package_path(value, "ExecActivity")?;
                    command = Some(ServiceCommand::PackageActivity { package, activity });
                }
                "ExecPackageBinary" => {
                    let (package, content_path) = extract_package_path(value, "ExecPackageBinary")?;
                    command = Some(ServiceCommand::LaunchPackageBinary {
                        package,
                        content_path,
                        args: None,
                        trigger_activity: None,
                    });
                }
                "ExecJvmClass" => {
                    let (package, class) = extract_package_path(value, "ExecJvmClass")?;
                    command = Some(ServiceCommand::JVMClass {
                        package,
                        class,
                        command_args: None,
                        jvm_args: None,
                        trigger_activity: None,
                    });
                }
                "JvmArgs" => extra_jvm_args = Some(value.trim().to_string()),
                "ExecArgs" => extra_command_args = Some(value.trim().to_string()),
                "TriggerActivity" => {
                    let (package, activity) = extract_package_path(value, "TriggerActivity")?;
                    trigger_app = Some(ExploitTriggerActivity { package, activity });
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

        let command = if let Some(mut command) = command {
            match command {
                ServiceCommand::Command {
                    ref command,
                    ref mut trigger_activity,
                } => {
                    if command.is_empty() {
                        return Err(Error::ConfigError("\"Exec\" cannot be empty".into()));
                    }

                    if let Some(activity) = trigger_app {
                        trigger_activity.replace(activity);
                    }
                }
                ServiceCommand::LaunchPackageBinary {
                    ref package,
                    ref content_path,
                    ref mut args,
                    ref mut trigger_activity,
                } => {
                    if package.is_empty() {
                        return Err(Error::ConfigError(
                            "\"ExecPackageBinary\" must contain a package".into(),
                        ));
                    }

                    if content_path.is_empty() {
                        return Err(Error::ConfigError(
                            "\"ExecPackageBinary\" must contain a content path".into(),
                        ));
                    }

                    if let Some(extra_args) = extra_command_args {
                        args.replace(extra_args);
                    }

                    if let Some(activity) = trigger_app {
                        trigger_activity.replace(activity);
                    }
                }
                ServiceCommand::PackageActivity {
                    ref package,
                    ref activity,
                } => {
                    if package.is_empty() {
                        return Err(Error::ConfigError(
                            "\"ExecActivity\" must contain a package".into(),
                        ));
                    }

                    if activity.is_empty() {
                        return Err(Error::ConfigError(
                            "\"ExecActivity\" must contain an activity".into(),
                        ));
                    }
                }
                ServiceCommand::JVMClass {
                    ref package,
                    ref class,
                    ref mut command_args,
                    ref mut jvm_args,
                    ref mut trigger_activity,
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

                    if let Some(extra_args) = extra_command_args {
                        command_args.replace(extra_args);
                    }

                    if let Some(extra_jvm_args) = extra_jvm_args {
                        jvm_args.replace(extra_jvm_args);
                    }

                    if let Some(activity) = trigger_app {
                        trigger_activity.replace(activity);
                    }
                }
            }

            command
        } else {
            return Err(Error::ConfigError(
                "\"Exec\", \"ExecPackageBinary\", or \"ExecJvmClass\" must be provided".into(),
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
            dependencies,
        })
    }
}

fn extract_package_path(value: &str, field_name: &str) -> Result<(String, String)> {
    let mut iter = value.trim().splitn(2, "/");
    let package = iter.next();
    let content_path = iter.next();

    match (package, content_path) {
        (Some(package), Some(content_path)) => Ok((package.into(), content_path.into())),
        (None, _) => Err(Error::ConfigError(format!(
            "Could not parse {field_name}: No package"
        ))),
        (_, None) => Err(Error::ConfigError(format!(
            "Could not parse {field_name}: No content path"
        ))),
    }
}
