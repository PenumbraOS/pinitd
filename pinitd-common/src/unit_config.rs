use std::{fmt::Display, path::PathBuf};

use dependency_graph::Node;
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
pub enum ServiceCommandKind {
    /// Launches an arbitrary command
    Command {
        command: String,
        trigger_activity: Option<ExploitTriggerActivity>,
    },
    /// Launches a binary contained within an APK. Will look up the APK path, then apply `content_path` on top of that to find the binary to launch
    LaunchPackageBinary {
        package: String,
        content_path: String,
        args: Option<String>,
        trigger_activity: Option<ExploitTriggerActivity>,
    },
    /// Launches a normal Android Activity directly through AMS. Does not rely on the Zygote vulnerability
    PackageActivity { package: String, activity: String },
    /// Launches a JVM process using `app_process`. The classpath will be set to the package APK. Does not provide a full Android app context
    JVMClass {
        package: String,
        class: String,
        command_args: Option<String>,
        jvm_args: Option<String>,
        trigger_activity: Option<ExploitTriggerActivity>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ServiceCommand {
    pub kind: ServiceCommandKind,
    pub uid: UID,
}

impl Display for ServiceCommandKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceCommandKind::Command { command, .. } => {
                f.write_fmt(format_args!("Command: {command}"))
            }
            ServiceCommandKind::LaunchPackageBinary {
                package,
                content_path,
                ..
            } => f.write_fmt(format_args!(
                "Package binary command: {content_path} at {package}"
            )),
            ServiceCommandKind::PackageActivity { package, activity } => {
                f.write_fmt(format_args!("Package activity: {package}/{activity}"))
            }
            ServiceCommandKind::JVMClass { package, class, .. } => {
                f.write_fmt(format_args!("JVM class command: {class} at {package}"))
            }
        }
    }
}

impl Display for ServiceCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{} (uid: {:?})", self.kind, self.uid))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ServiceConfig {
    pub name: String,
    pub command: ServiceCommand,
    pub autostart: bool,
    pub restart: RestartPolicy,
    pub se_info: Option<String>,
    pub nice_name: Option<String>,
    pub unit_file_path: PathBuf,
    pub dependencies: ServiceDependencies,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct ServiceDependencies {
    pub wants: Vec<String>,
}

impl Node for ServiceConfig {
    type DependencyType = String;

    fn dependencies(&self) -> &[Self::DependencyType] {
        &self.dependencies.wants
    }

    fn matches(&self, dependency: &Self::DependencyType) -> bool {
        &self.name == dependency
    }
}
