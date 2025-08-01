use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime},
};

use crate::{
    worker::protocol::{ServiceState, WorkerState},
    wrapper::daemonize,
};
use pinitd_common::{ServiceRunState, UID, WORKER_CONTROLLER_POLL_INTERVAL, WorkerIdentity};
use tokio::{
    process::Command,
    select,
    sync::Mutex,
    time::{interval, sleep},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    error::Result,
    worker::{
        connection::ControllerConnection,
        protocol::{WorkerCommand, WorkerEvent, WorkerMessage, WorkerResponse},
    },
};

/// Comprehensive process tracking information
#[allow(dead_code)]
#[derive(Debug)]
pub struct ProcessInfo {
    /// Process ID
    pub pid: u32,
    /// Service name this process belongs to
    pub service_name: String,
    /// Unique identifier for this service instance
    pub pinit_id: Uuid,
    /// Command line that was used to start the process
    pub command_line: String,
    /// Time when the process was started
    pub start_time: SystemTime,
}

impl ProcessInfo {
    pub fn new(pid: u32, service_name: String, pinit_id: Uuid, command_line: String) -> Self {
        Self {
            pid,
            service_name,
            pinit_id,
            command_line,
            start_time: SystemTime::now(),
        }
    }
}

pub struct WorkerProcess;

impl WorkerProcess {
    pub fn specialize(uid: UID, se_info: Option<String>) -> Result<()> {
        daemonize(async move {
            info!("Worker started for UID {uid:?}, se_info: {se_info:?}");

            let start_time = SystemTime::now();
            let running_processes = Arc::new(Mutex::new(HashMap::<String, ProcessInfo>::new()));
            let worker_se_info = se_info.unwrap_or_else(|| WorkerIdentity::default_se_info(&uid));

            loop {
                match ControllerConnection::open().await {
                    Ok(connection) => {
                        // Send worker identification as first message
                        let worker_pid = std::process::id() as usize;
                        let identification = WorkerEvent::WorkerRegistration {
                            worker_uid: uid.clone(),
                            worker_pid,
                            worker_se_info: worker_se_info.clone(),
                        };
                        if let Err(e) = connection
                            .write_response(WorkerMessage::Event(identification))
                            .await
                        {
                            error!("Failed to send worker identification: {}", e);
                            continue;
                        }

                        // Run normal worker loop with this connection
                        if let Err(e) = Self::connection_command_loop(
                            uid.clone(),
                            start_time,
                            running_processes.clone(),
                            connection,
                        )
                        .await
                        {
                            warn!("Worker connection lost: {}", e);
                        }
                    }
                    Err(_) => {
                        sleep(WORKER_CONTROLLER_POLL_INTERVAL).await;
                    }
                }
            }
        })
    }

    async fn connection_command_loop(
        uid: UID,
        start_time: SystemTime,
        running_processes: Arc<Mutex<HashMap<String, ProcessInfo>>>,
        mut connection: ControllerConnection,
    ) -> Result<()> {
        let token = CancellationToken::new();
        // Send heartbeat every 30 seconds
        let mut heartbeat_interval = interval(Duration::from_secs(30));

        loop {
            select! {
                _ = token.cancelled() => {
                    warn!("Worker shutting down");
                    break;
                }
                _ = heartbeat_interval.tick() => {
                    let uptime = start_time.elapsed().unwrap_or(Duration::ZERO).as_secs();
                    let active_services = running_processes.lock().await.len() as u32;
                    let heartbeat = WorkerEvent::Heartbeat {
                        worker_uid: uid.clone(),
                        uptime_seconds: uptime,
                        active_services,
                    };

                    if let Err(e) = connection.write_response(WorkerMessage::Event(heartbeat)).await {
                        error!("Failed to send heartbeat: {}", e);
                        return Err(e); // Connection lost
                    }
                }
                result = connection.read_command() => match result {
                    Ok(command) => {
                        info!("Received command {command:?}");

                        let response = match handle_command(command, &running_processes, &connection, &uid).await {
                            Ok(response) => response,
                            Err(err) => {
                                let err = format!("Error processing command: {err}");
                                error!("{err}");
                                WorkerResponse::Error(err)
                            }
                        };

                        info!("Sending command response");
                        if let Err(e) = connection.write_response(WorkerMessage::Response(response)).await {
                            error!("Failed to send response: {}", e);
                        }
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

    /// Spawn a remote process to act as a worker for a specific UID
    #[cfg(target_os = "android")]
    pub async fn spawn(identity: &WorkerIdentity, launch_package: Option<String>) -> Result<()> {
        // Controller runs in Shell. Shell worker spawns as child, others via Zygote
        if identity.uid == UID::Shell {
            let process_path = std::env::args().next().unwrap();
            let mut cmd = tokio::process::Command::new(process_path);
            cmd.arg("worker").arg("--uid").arg("2000");
            cmd.spawn()?;
            return Ok(());
        }

        use crate::exploit::exploit;
        use android_31317_exploit::{ExploitKind, TriggerApp};
        use std::env;

        let executable = env::current_exe()?;
        let executable = executable.display();

        // Convert UID to numeric value for exploit
        let uid_arg = match &identity.uid {
            UID::System => "1000".to_string(),
            UID::Shell => unreachable!("Shell handled above"),
            UID::Custom(uid_num) => uid_num.to_string(),
        };

        let uid_num: usize = identity.uid.clone().into();

        let worker_args = format!(
            "{executable} worker --uid {uid_arg} --se-info {}",
            identity.se_info
        );

        let payload = exploit()?.new_launch_payload(
            uid_num,
            None,
            None,
            "/data/",
            &launch_package.unwrap_or("com.android.settings".into()),
            &identity.se_info,
            &ExploitKind::Command(format!(
                "{executable} internal-wrapper --is-zygote \"{worker_args}\""
            )),
            None,
        )?;

        payload
            .execute(
                &TriggerApp::new(
                    "com.android.settings".into(),
                    "com.android.settings.Settings".into(),
                ),
                true,
                true,
            )
            .await?;

        Ok(())
    }

    /// Spawn a remote process to act as a worker for a specific UID
    #[cfg(not(target_os = "android"))]
    pub async fn spawn(identity: &WorkerIdentity, _launch_package: Option<String>) -> Result<()> {
        let process_path = std::env::args().next().unwrap();
        let mut cmd = tokio::process::Command::new(process_path);
        cmd.arg("worker");

        // Add UID as argument
        let uid_arg = match identity.uid {
            UID::System => "1000",
            UID::Shell => "2000",
            UID::Custom(uid_num) => {
                cmd.arg("--uid").arg(uid_num.to_string());
                cmd.spawn()?;
                return Ok(());
            }
        };

        cmd.arg("--uid").arg(uid_arg);
        cmd.spawn()?;
        Ok(())
    }
}

async fn handle_command(
    command: WorkerCommand,
    running_processes: &Arc<Mutex<HashMap<String, ProcessInfo>>>,
    connection: &ControllerConnection,
    worker_uid: &UID,
) -> Result<WorkerResponse> {
    match command {
        WorkerCommand::SpawnProcess {
            command,
            pinit_id,
            service_name,
        } => {
            info!(
                "Spawning process for service '{}': {}",
                service_name, command
            );

            let mut child = Command::new("sh").args(["-c", &command]).spawn()?;

            let pid = match child.id() {
                Some(pid) => pid,
                None => {
                    if let Ok(Some(exit)) = child.try_wait() {
                        let _ = connection
                            .write_response(WorkerMessage::Event(WorkerEvent::ProcessExited {
                                service_name,
                                exit_code: exit.code().unwrap_or(-1),
                            }))
                            .await;
                    }

                    return Ok(WorkerResponse::Success);
                }
            };

            let process_info =
                ProcessInfo::new(pid, service_name.clone(), pinit_id, command.clone());
            running_processes
                .lock()
                .await
                .insert(service_name.clone(), process_info);

            // Notify controller that process spawned successfully
            let spawn_event: WorkerEvent = WorkerEvent::ProcessSpawned {
                service_name: service_name.clone(),
                pinit_id,
            };
            let _ = connection
                .write_response(WorkerMessage::Event(spawn_event))
                .await;

            // Start monitoring this process
            let service_name_clone = service_name.clone();
            let connection_clone = connection.clone();
            let running_processes_clone = running_processes.clone();
            tokio::spawn(async move {
                match child.wait().await {
                    Ok(exit_status) => {
                        let exit_code = exit_status.code().unwrap_or(-1);
                        info!(
                            "Process for service '{}' exited with code {}",
                            service_name_clone, exit_code
                        );

                        // Remove from tracking
                        running_processes_clone
                            .lock()
                            .await
                            .remove(&service_name_clone);

                        let _ = connection_clone
                            .write_response(WorkerMessage::Event(WorkerEvent::ProcessExited {
                                service_name: service_name_clone,
                                exit_code,
                            }))
                            .await;
                    }
                    Err(e) => {
                        error!("Error waiting for process '{}': {}", service_name_clone, e);

                        // Remove from tracking on error too
                        running_processes_clone
                            .lock()
                            .await
                            .remove(&service_name_clone);

                        let _ = connection_clone
                            .write_response(WorkerMessage::Event(WorkerEvent::WorkerError {
                                service_name: Some(service_name_clone),
                                error: format!("Process wait error: {}", e),
                            }))
                            .await;
                    }
                }
            });
        }
        WorkerCommand::KillProcess { service_name } => {
            info!("Killing process for service '{}'", service_name);

            if let Some(process_info) = running_processes.lock().await.remove(&service_name) {
                // Kill the process using the PID
                use std::process::Command;
                let _ = Command::new("kill")
                    .args(["-TERM", &process_info.pid.to_string()])
                    .output();
            } else {
                // Process might not be in our tracking (if spawned via different method)
                warn!("Process '{}' not found in running processes", service_name);
            }
        }
        WorkerCommand::Status => {
            // Return status of running processes
            let mut status = HashMap::new();
            let processes = running_processes.lock().await;

            for (name, process_info) in processes.iter() {
                status.insert(
                    name.clone(),
                    pinitd_common::ServiceRunState::Running {
                        pid: Some(process_info.pid),
                    },
                );
            }

            return Ok(WorkerResponse::Status(status));
        }
        WorkerCommand::Shutdown => {
            info!("Worker shutdown requested");

            // Kill all running processes
            for (name, process_info) in running_processes.lock().await.drain() {
                info!(
                    "Killing process for service '{}' (PID: {})",
                    name, process_info.pid
                );
                use std::process::Command;
                let _ = Command::new("kill")
                    .args(["-TERM", &process_info.pid.to_string()])
                    .output();
            }

            return Ok(WorkerResponse::ShuttingDown);
        }
        WorkerCommand::CGroupReparentCommand { pid } => {
            if *worker_uid != UID::System {
                error!("Received CGroupReparentCommand on non-system worker {worker_uid:?}");
            }

            info!("Executing cgroup reparent for PID {pid:?}");

            return match Command::new("sh")
                .args([
                    "-c",
                    &format!("echo {pid} > /sys/fs/cgroup/uid_1000/cgroup.procs"),
                ])
                .output()
                .await
            {
                Ok(output) => {
                    if output.status.success() {
                        info!("cgroup reparent executed successfully");
                        Ok(WorkerResponse::Success)
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let err = format!("cgroup reparent failed: {stderr}");
                        error!("{err}");
                        Ok(WorkerResponse::Error(err))
                    }
                }
                Err(err) => {
                    let err = format!("Failed to execute cgroup reparent command: {err}");
                    error!("{err}");
                    Ok(WorkerResponse::Error(err))
                }
            };
        }
        WorkerCommand::RequestCurrentState => {
            info!("Received request for current worker state");

            let processes = running_processes.lock().await;
            let mut services = Vec::new();

            for (name, process_info) in processes.iter() {
                let service_state = ServiceState {
                    service_name: name.clone(),
                    run_state: ServiceRunState::Running {
                        pid: Some(process_info.pid),
                    },
                };
                services.push(service_state);
            }

            let worker_state = WorkerState { services };
            return Ok(WorkerResponse::CurrentState(worker_state));
        }
    };

    Ok(WorkerResponse::Success)
}
