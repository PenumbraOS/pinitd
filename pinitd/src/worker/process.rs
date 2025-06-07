use std::{collections::HashMap, process, sync::Arc, time::Duration};

use tokio::{
    select,
    sync::{Mutex, broadcast},
    time::{sleep, timeout},
};
use tokio_util::sync::CancellationToken;

use crate::{
    error::Result,
    registry::{Registry, local::LocalRegistry},
    worker::{
        connection::ControllerConnection,
        protocol::{WorkerCommand, WorkerResponse},
    },
};

use super::connection::WorkerConnectionStatus;

pub struct WorkerProcess;

impl WorkerProcess {
    pub async fn specialize() -> Result<()> {
        info!("Connecting to controller");

        let mut connection = ControllerConnection::open().await?;
        info!("Controller connected");

        let mut registry = LocalRegistry::new_worker(connection.clone())?;
        let token = CancellationToken::new();

        loop {
            select! {
                _ = token.cancelled() => {
                    warn!("Worker shutting down");
                    break;
                }
                result = connection.read_command() => match result {
                    Ok(command) => {
                        info!("Received command {command:?}");

                        // Before processing command, open lock on write socket so we don't push any data in the middle of our command response
                        let write_lock = connection.acquire_write_lock().await;

                        let response = match handle_command(command, &mut registry, &token).await {
                            Ok(response) => response,
                            Err(err) => {
                                let err = format!("Error processing command: {err}");
                                error!("{err}");
                                WorkerResponse::Error(err)
                            }
                        };

                        info!("Sending command response");
                        connection.write_response_with_lock(write_lock, response).await?;
                    }
                    Err(err) => {
                        error!("Error processing command packet: {err}");
                        info!("Reconnecting to controller");
                        connection = ControllerConnection::open().await?;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn spawn_with_retries(
        retry_count: usize,
        worker_connected_rx: broadcast::Receiver<WorkerConnectionStatus>,
    ) -> Result<()> {
        if let Err(error) = Self::spawn() {
            error!("Failed worker spawn {error}");
        }

        tokio::spawn(async move {
            for i in 0..retry_count {
                let mut inner_rx = worker_connected_rx.resubscribe();
                let did_complete = Arc::new(Mutex::new(false));
                let inner_did_complete = did_complete.clone();
                let _ = timeout(Duration::from_millis(500), async move {
                    if let WorkerConnectionStatus::Connected(_) =
                        WorkerConnectionStatus::await_update(&mut inner_rx).await
                    {
                        // We've succeeded
                        *inner_did_complete.lock().await = true;
                    }
                })
                .await;

                if *did_complete.lock().await {
                    return;
                }

                let will_retry = i < retry_count - 1;
                let retry_string = if will_retry {
                    format!("Retrying ({}/{retry_count})", i + 1)
                } else {
                    "Killing".into()
                };

                error!("Failed to connect to worker. {retry_string}");

                if !will_retry {
                    process::exit(1);
                }

                sleep(Duration::from_secs(5)).await;

                let _ = Self::spawn();
            }
        });

        Ok(())
    }

    /// Spawn a remote process to act as the system worker
    #[cfg(target_os = "android")]
    fn spawn() -> Result<()> {
        use android_31317_exploit::{ExploitKind, TriggerApp, build_and_execute};
        use std::env;

        let executable = env::current_exe()?;
        let executable = executable.display();

        build_and_execute(
            1000,
            None,
            "/data/",
            "com.android.settings",
            "platform:system_app:targetSdkVersion=29:complete",
            &ExploitKind::Command(format!(
                "{executable} internal-wrapper --is-zygote \"{executable} worker\""
            )),
            &TriggerApp::new(
                "com.android.settings".into(),
                "com.android.settings.Settings".into(),
            ),
            None,
            true,
        )?;

        Ok(())
    }

    /// Spawn a remote process to act as the system worker
    #[cfg(not(target_os = "android"))]
    fn spawn() -> Result<()> {
        let process_path = std::env::args().next().unwrap();
        tokio::process::Command::new(process_path)
            .arg("worker")
            .spawn()?;
        Ok(())
    }
}

async fn handle_command(
    command: WorkerCommand,
    registry: &mut LocalRegistry,
    token: &CancellationToken,
) -> Result<WorkerResponse> {
    match command {
        WorkerCommand::Create(service_config) => {
            // Register config
            registry.insert_unit(service_config, true).await?;
        }
        WorkerCommand::Destroy(name) => {
            // Delete config
            registry.remove_unit(name).await?;
        }
        WorkerCommand::Start {
            service_name,
            pinit_id,
        } => {
            registry
                .service_start_with_id(service_name, pinit_id)
                .await?;
        }
        WorkerCommand::Stop(name) => {
            registry.service_stop(name).await?;
        }
        WorkerCommand::Restart {
            service_name,
            pinit_id,
        } => {
            registry
                .service_restart_with_id(service_name, pinit_id)
                .await?;
        }
        WorkerCommand::Status => {
            let status = registry.service_list_all().await?;
            let status_iter = status.into_iter().map(|s| (s.name, s.state));
            return Ok(WorkerResponse::Status(HashMap::from_iter(status_iter)));
        }
        WorkerCommand::Shutdown => {
            let _ = registry.shutdown().await;
            // Trigger process shutdown
            token.cancel();
            return Ok(WorkerResponse::ShuttingDown);
        }
    };

    Ok(WorkerResponse::Success)
}
