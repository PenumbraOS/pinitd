use std::{env, future, path::PathBuf, process::Stdio};

use crate::{
    android::fetch_package_path,
    error::{Error, Result},
    exploit::exploit,
};
use android_31317_exploit::{ExploitKind, TriggerApp};
use pinitd_common::{
    ServiceRunState, UID,
    unit_config::{ServiceCommand, ServiceCommandKind, ServiceConfig},
};
use tokio::process::{Child, Command};
use uuid::Uuid;

use super::local::LocalRegistry;

pub struct SpawnCommand {
    pub exit_code: i32,
    pub exit_message: String,
}

impl SpawnCommand {
    pub async fn spawn(
        registry: LocalRegistry,
        name: String,
        pinit_id: Uuid,
        force_zygote_spawn: bool,
    ) -> Result<Self> {
        let config = registry
            .with_service(&name, |service| Ok(service.config().clone()))
            .await?;

        info!("Spawning process for \"{name}\": \"{}\"", config.command);

        let (command, force_standard_spawn) = match expanded_command(&config.command).await {
            Ok(result) => result,
            Err(err) => {
                error!("Failed to build process spawn path: {err}");
                return Err(err);
            }
        };

        let child = if !force_standard_spawn
            && ((config.command.uid != UID::Shell && config.command.uid != UID::System)
                || force_zygote_spawn)
        {
            info!("Launching \"{name}\" via Zygote");
            spawn_zygote_exploit(config, command, pinit_id).await
        } else {
            info!("Launching \"{name}\" via normal spawn");
            spawn_standard(command, pinit_id).await
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

async fn spawn_standard(command: String, pinit_id: Uuid) -> Result<InnerSpawnChild> {
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

async fn spawn_zygote_exploit(
    config: ServiceConfig,
    command: String,
    pinit_id: Uuid,
) -> Result<InnerSpawnChild> {
    let command = wrapper_command(&command, pinit_id, true)?;
    let trigger_app = zygote_trigger_activity(&config.command);

    let payload = exploit()?.new_launch_payload(
        config.command.uid.into(),
        None,
        Some(3003),
        "/data/",
        "com.android.settings",
        config.se_info.as_ref().map_or(
            "platform:system_app:targetSdkVersion=29:complete",
            |se_info| &se_info,
        ),
        &ExploitKind::Command(command),
        config.nice_name.as_deref(),
    )?;

    payload.execute(&trigger_app, true, true).await?;

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

async fn expanded_command(command: &ServiceCommand) -> Result<(String, bool)> {
    let command = match &command.kind {
        ServiceCommandKind::Command { command, .. } => command.clone(),
        ServiceCommandKind::LaunchPackageBinary {
            package,
            content_path,
            args,
            ..
        } => {
            let package_path = fetch_package_path(&package).await?;
            let path = PathBuf::from(package_path);
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

            command
        }
        ServiceCommandKind::PackageActivity { package, activity } => {
            let command = format!("am start -n {package}/{activity}");
            return Ok((command, true));
        }
        ServiceCommandKind::JVMClass {
            package,
            class,
            command_args,
            jvm_args,
            ..
        } => {
            let package_path = fetch_package_path(&package).await?;

            let args = if let Some(command_args) = command_args {
                command_args
            } else {
                ""
            };

            let jvm_args = if let Some(jvm_args) = jvm_args {
                jvm_args
            } else {
                ""
            };

            format!(
                "/system/bin/app_process -cp {package_path} {jvm_args} /system/bin --application {class} {args}"
            ).trim().to_string()
        }
    };

    Ok((command, false))
}

fn zygote_trigger_activity(command: &ServiceCommand) -> TriggerApp {
    let trigger_activity = match &command.kind {
        ServiceCommandKind::Command {
            trigger_activity, ..
        } => trigger_activity,
        ServiceCommandKind::LaunchPackageBinary {
            trigger_activity, ..
        } => trigger_activity,
        ServiceCommandKind::PackageActivity { .. } => &None,
        ServiceCommandKind::JVMClass {
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
