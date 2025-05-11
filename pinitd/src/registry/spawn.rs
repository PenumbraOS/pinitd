use std::{future, process::Stdio};

use crate::error::{Error, Result};
use android_31317_exploit::exploit::{ExploitKind, TriggerApp, build_and_execute};
use pinitd_common::{ServiceRunState, UID, unit::ServiceConfig};
use tokio::process::{Child, Command};

use super::local::LocalRegistry;

pub struct SpawnCommand {
    pub exit_code: i32,
    pub exit_message: String,
}

impl SpawnCommand {
    pub async fn spawn(registry: LocalRegistry, name: String) -> Result<Self> {
        let config = registry
            .with_service(&name, |service| Ok(service.config().clone()))
            .await?;

        info!("Spawning process for \"{name}\": \"{}\"", config.command);

        let child = if config.uid == UID::System && config.nice_name.is_some() {
            spawn_zygote_exploit(config)
        } else {
            spawn_standard(config)
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
    fn pid(&self, name: &str) -> Result<i32> {
        match self {
            InnerSpawnChild::Standard(child) => child.id().map_or_else(
                || {
                    Err(Error::Unknown(format!(
                        "Failed to get PID for spawned process \"{name}\"",
                    )))
                },
                |pid| Ok(pid as i32),
            ),
            // TODO: Provide pid reporting method for Zygote processes
            InnerSpawnChild::ZygoteExploit => Ok(100000),
        }
    }

    async fn wait(&mut self, name: &str) -> SpawnCommand {
        match self {
            InnerSpawnChild::Standard(child) => match child.wait().await {
                Ok(status) => {
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

fn spawn_standard(config: ServiceConfig) -> Result<InnerSpawnChild> {
    let child = Command::new("sh")
        .args(&["-c", &config.command])
        // TODO: Auto pipe output to Android log?
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Make sure we clean up if we die
        .kill_on_drop(true)
        .spawn()?;

    Ok(InnerSpawnChild::Standard(child))
}

fn spawn_zygote_exploit(config: ServiceConfig) -> Result<InnerSpawnChild> {
    build_and_execute(
        config.uid as usize,
        "/data/",
        "com.android.settings",
        "platform:system_app:targetSdkVersion=29:complete",
        &ExploitKind::Command(config.command),
        &TriggerApp::new(
            "com.android.settings".into(),
            "com.android.settings.Settings".into(),
        ),
        config.nice_name.as_deref(),
    )?;

    Ok(InnerSpawnChild::ZygoteExploit)
}
