use std::{env, future, path::PathBuf, process::Stdio, time::Duration};

use crate::error::{Error, Result};
use android_31317_exploit::{ExploitKind, TriggerApp, build_and_execute};
use pinitd_common::{
    ServiceRunState, UID,
    unit_config::{ServiceCommand, ServiceConfig},
};
use tokio::{
    process::{Child, Command},
    time::timeout,
};
use uuid::Uuid;

use super::local::LocalRegistry;

pub struct SpawnCommand {
    pub exit_code: i32,
    pub exit_message: String,
}

impl SpawnCommand {
    pub async fn spawn(registry: LocalRegistry, name: String, pinit_id: Uuid) -> Result<Self> {
        let config = registry
            .with_service(&name, |service| Ok(service.config().clone()))
            .await?;

        info!("Spawning process for \"{name}\": \"{}\"", config.command);

        let child = if config.uid != UID::Shell && config.uid != UID::System {
            spawn_zygote_exploit(config, pinit_id).await
        } else {
            spawn_standard(config, pinit_id).await
        };

        match child {
            Ok(mut child) => {
                let pid = child.pid(&name)?;
                registry
                    .with_service_mut(&name, |service| {
                        service.set_state(ServiceRunState::Running { pid });
                        Ok(())
                    })
                    .await?;

                info!("Monitoring task started for service \"{name}\"");
                let result = child.wait(&name).await;
                info!("Monitoring task finished for service \"{name}\"");

                Ok(result)
            }
            Err(err) => {
                let error_msg = format!("Failed to spawn process for \"{name}\": {err}");
                error!("{}", error_msg);
                registry
                    .with_service_mut(&name, |service| {
                        service.set_state(ServiceRunState::Failed {
                            reason: error_msg.clone(),
                        });
                        Ok(())
                    })
                    .await?;
                Err(Error::ProcessSpawnError(error_msg))
            }
        }
    }
}

enum InnerSpawnChild {
    Standard(Child),
    ZygoteExploit,
}

impl InnerSpawnChild {
    fn pid(&self, name: &str) -> Result<u32> {
        match self {
            InnerSpawnChild::Standard(child) => child.id().map_or_else(
                || {
                    Err(Error::Unknown(format!(
                        "Failed to get PID for spawned process \"{name}\"",
                    )))
                },
                |pid| Ok(pid),
            ),
            // TODO: Provide pid reporting method for Zygote processes
            InnerSpawnChild::ZygoteExploit => Ok(100000),
        }
    }

    async fn wait(&mut self, name: &str) -> SpawnCommand {
        match self {
            InnerSpawnChild::Standard(child) => match child.wait().await {
                Ok(status) => {
                    // TODO: This status code is of the wrapper, not the service
                    info!("Process for service \"{name}\" exited with status: {status}",);
                    status.code().map_or_else(
                        || SpawnCommand {
                            exit_code: 127,
                            exit_message: "Exited via signal".into(),
                        },
                        |code| SpawnCommand {
                            exit_code: code,
                            exit_message: format!("Exited with code {code}"),
                        },
                    )
                }
                Err(err) => {
                    error!("Error waiting on process for service \"{name}\": {err}");
                    SpawnCommand {
                        exit_code: 127,
                        exit_message: format!("Wait error: {err}"),
                    }
                }
            },
            InnerSpawnChild::ZygoteExploit => future::pending().await,
        }
    }
}

async fn spawn_standard(config: ServiceConfig, pinit_id: Uuid) -> Result<InnerSpawnChild> {
    let command = expanded_command(&config.command).await?;
    let command = wrapper_command(&command, pinit_id, false)?;

    let child = Command::new("sh")
        .args(&["-c", &command])
        // TODO: Auto pipe output to Android log?
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Make sure we clean up if we die
        .kill_on_drop(true)
        .spawn()?;

    Ok(InnerSpawnChild::Standard(child))
}

async fn spawn_zygote_exploit(config: ServiceConfig, pinit_id: Uuid) -> Result<InnerSpawnChild> {
    let command = expanded_command(&config.command).await?;
    let command = wrapper_command(&command, pinit_id, true)?;
    let trigger_app = zygote_trigger_activity(&config.command);

    build_and_execute(
        config.uid.into(),
        None,
        "/data/",
        "com.android.settings",
        config.se_info.as_ref().map_or(
            "platform:system_app:targetSdkVersion=29:complete",
            |se_info| &se_info,
        ),
        &ExploitKind::Command(command),
        &trigger_app,
        config.nice_name.as_deref(),
        true,
        true,
    )
    .await?;

    Ok(InnerSpawnChild::ZygoteExploit)
}

fn wrapper_command(command: &str, pinit_id: Uuid, is_zygote: bool) -> Result<String> {
    let path = env::current_exe()?;
    let zygote_arg = if is_zygote { "--is-zygote " } else { "" };
    Ok(format!(
        "{} monitored-wrapper {zygote_arg}\"{pinit_id}\" \"{command}\"",
        path.display()
    ))
}

async fn expanded_command(command: &ServiceCommand) -> Result<String> {
    match command {
        ServiceCommand::Command { command, .. } => Ok(command.clone()),
        ServiceCommand::LaunchPackage {
            package,
            content_path,
            args,
            ..
        } => {
            let package_path = fetch_package_path(package).await?;
            let path = PathBuf::from(&package_path);
            let path = path.join(
                content_path
                    .strip_prefix("/")
                    .unwrap_or_else(|| &content_path),
            );

            let command = path.display().to_string();

            let command = if let Some(args) = args {
                format!("{command} {args}").trim().to_string()
            } else {
                command
            };

            Ok(command)
        }
        ServiceCommand::JVMClass {
            package,
            class,
            command_args,
            jvm_args,
            ..
        } => {
            let package_path = fetch_package_path(package).await?;

            let args = if let Some(command_args) = command_args {
                &command_args
            } else {
                ""
            };

            let jvm_args = if let Some(jvm_args) = jvm_args {
                &jvm_args
            } else {
                ""
            };

            Ok(format!(
                "/system/bin/app_process -cp {package_path} {jvm_args} /system/bin --application {class} {args}"
            ).trim().to_string())
        }
    }
}

fn zygote_trigger_activity(command: &ServiceCommand) -> TriggerApp {
    let trigger_activity = match command {
        ServiceCommand::Command {
            trigger_activity, ..
        } => trigger_activity,
        ServiceCommand::LaunchPackage {
            trigger_activity, ..
        } => trigger_activity,
        ServiceCommand::JVMClass {
            trigger_activity, ..
        } => trigger_activity,
    }
    .clone();

    trigger_activity.map_or(
        TriggerApp::new(
            "com.android.settings".into(),
            "com.android.settings.Settings".into(),
        ),
        |trigger| TriggerApp::new(trigger.package.clone(), trigger.package.clone()),
    )
}

async fn fetch_package_path(package: &str) -> Result<String> {
    let child = Command::new("pm")
        .args(&["path", package])
        // TODO: Auto pipe output to Android log?
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let output = timeout(Duration::from_millis(500), child.wait_with_output()).await??;

    if !output.status.success() {
        return Err(Error::ProcessSpawnError(format!(
            "Could not find package {package}"
        )));
    }

    let stdout = String::from_utf8(output.stdout).ok();

    if let Some(stdout) = stdout {
        let package_path = stdout.trim_start_matches("package:").trim();
        if !package_path.starts_with("/data/app") {
            return Err(Error::ProcessSpawnError(format!(
                "Found invalid package path for package {package}. Found {package_path}"
            )));
        }

        return Ok(package_path.into());
    }

    Err(Error::ProcessSpawnError(format!(
        "Could not find package {package}"
    )))
}
