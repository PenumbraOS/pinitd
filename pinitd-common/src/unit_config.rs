use std::{fmt::Display, path::PathBuf};

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
pub struct ExploitTriggerActivity {
    pub package: String,
    pub activity: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ServiceCommand {
    /// Launches an arbitrary command
    Command {
        command: String,
        trigger_activity: Option<ExploitTriggerActivity>,
    },
    /// Launches a binary contained within an APK. Will look up the APK path, then apply `content_path` on top of that to find the binary to launch
    LaunchPackage {
        package: String,
        content_path: String,
        args: Option<String>,
        trigger_activity: Option<ExploitTriggerActivity>,
    },
    /// Launches a JVM process using `app_process`. The classpath will be set to the package APK. Does not provide a full Android app context
    JVMClass {
        package: String,
        class: String,
        command_args: Option<String>,
        jvm_args: Option<String>,
        trigger_activity: Option<ExploitTriggerActivity>,
    },
}

impl Display for ServiceCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceCommand::Command { command, .. } => {
                f.write_fmt(format_args!("Command: {command}"))
            }
            ServiceCommand::LaunchPackage {
                package,
                content_path,
                ..
            } => f.write_fmt(format_args!("Package command: {content_path} at {package}")),
            ServiceCommand::JVMClass { package, class, .. } => {
                f.write_fmt(format_args!("JVM class command: {class} at {package}"))
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ServiceConfig {
    pub name: String,
    pub command: ServiceCommand,
    pub autostart: bool,
    pub restart: RestartPolicy,
    pub uid: UID,
    pub se_info: Option<String>,
    pub nice_name: Option<String>,
    pub unit_file_path: PathBuf,
}
