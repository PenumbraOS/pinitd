use std::{process::Stdio, time::Duration};

use pinitd_common::ServiceRunState;
use tokio::{process::Command, task::JoinHandle, time::sleep};

use crate::{error::Error, types::ServiceRegistry, unit::RestartPolicy};

pub async fn spawn_and_monitor_service(
    name: String,
    registry: ServiceRegistry,
) -> Result<(), Error> {
    let mut registry_lock = registry.lock().await;
    let service = registry_lock
        .get_mut(&name)
        .ok_or_else(|| Error::Unknown(format!("Service \"{name}\" not found in registry")))?;

    if !service.enabled {
        warn!("Attempted to start disabled service \"{name}\". Ignoring.",);
        return Err(Error::Unknown(format!("Service \"{name}\" is disabled.")));
    }

    if let ServiceRunState::Running { .. } = service.state {
        info!("Service \"{name}\" is already running or starting.");
        return Ok(());
    }

    drop(service);
    drop(registry_lock);

    let handle = spawn(name.clone(), registry.clone());

    // Some time after start we should be able to acquire the lock to preserve this handle
    let mut registry_lock = registry.lock().await;
    let service = registry_lock
        .get_mut(&name)
        .ok_or_else(|| Error::Unknown(format!("Service \"{name}\" not found in registry")))?;

    service.monitor_task = Some(handle);

    Ok(())
}

fn spawn(name: String, registry: ServiceRegistry) -> JoinHandle<()> {
    let inner_name = name.clone();
    let inner_registry = registry.clone();
    tokio::spawn(async move {
        loop {
            if let Ok((code, message)) =
                perform_command(inner_name.clone(), inner_registry.clone()).await
            {
                if !should_restart(
                    inner_name.clone(),
                    inner_registry.clone(),
                    code != 0,
                    message,
                )
                .await
                {
                    // Terminate restart loop
                    return;
                }

                // Otherwise restart after delay
                sleep(Duration::from_millis(1000)).await;
            } else {
                // If error, terminate loop. It has already been logged
                return;
            }
        }
    })
}

async fn perform_command(name: String, registry: ServiceRegistry) -> Result<(i32, String), Error> {
    let mut registry_lock = registry.lock().await;
    let service = registry_lock
        .get_mut(&name)
        .ok_or_else(|| Error::Unknown(format!("Service \"{name}\" not found in registry")))?;

    let config = &service.config;

    info!(
        "Spawning process for {name}: {} {:?}",
        config.exec, config.args
    );

    let child = Command::new(config.exec.clone())
        .args(&config.args)
        // TODO: Auto pipe output to Android log?
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Make sure we clean up if we die
        .kill_on_drop(true)
        .spawn();

    match child {
        Ok(mut child) => {
            let pid = child.id().ok_or_else(|| {
                Error::Unknown(format!("Failed to get PID for spawned process \"{name}\"",))
            })?;
            info!("Service \"{name}\" spawned successfully with PID: {pid}",);

            service.state = ServiceRunState::Running { pid };

            // Drop contended resources before awaiting process
            drop(service);
            drop(registry_lock);

            info!("Monitoring task started for service \"{name}\"");
            let (exit_code, exit_message) = match child.wait().await {
                Ok(status) => {
                    info!("Process for service \"{name}\" exited with status: {status}",);
                    status.code().map_or_else(
                        || (127, "Exited via signal".to_string()),
                        |code| (code, format!("Exited with code {code}")),
                    )
                }
                Err(err) => {
                    error!("Error waiting on process for service \"{name}\": {err}");
                    (127, format!("Wait error: {err}"))
                }
            };

            info!("Monitoring task finished for service \"{name}\"");

            Ok((exit_code, exit_message))
        }
        Err(err) => {
            let error_msg = format!("Failed to spawn process for \"{name}\": {err}");
            error!("{}", error_msg);
            service.state = ServiceRunState::Failed {
                reason: error_msg.clone(),
            };
            Err(Error::Unknown(error_msg))
        }
    }
}

async fn should_restart(
    name: String,
    registry: ServiceRegistry,
    did_fail: bool,
    exit_message: String,
) -> bool {
    let mut registry_lock = registry.lock().await;
    if let Some(service) = registry_lock.get_mut(&name) {
        if did_fail {
            warn!("Service \"{name}\" transitioned to Failed state with message {exit_message}");
            service.state = ServiceRunState::Failed {
                reason: exit_message.clone(),
            };
        } else {
            info!("Service \"{name}\" transitioned to Stopped state");
            service.state = ServiceRunState::Stopped;
        }

        let should_restart: bool = service.config.restart == RestartPolicy::Always
            || (did_fail && service.config.restart == RestartPolicy::OnFailure);
        if service.enabled && should_restart {
            warn!("Restarting service \"{name}\" due to exit: {exit_message}");

            return true;
        } else if !service.enabled {
            info!("Service \"{name}\" exited but is disabled, not restarting");
        } else {
            info!("Service \"{name}\" exited and restart is not configured");
        }

        false
    } else {
        error!("Service \"{name}\" disappeared from registry during monitoring!",);

        false
    }
}
