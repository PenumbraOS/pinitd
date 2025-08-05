use pinitd_common::ServiceRunState;
use tokio::sync::mpsc;

use crate::{
    error::Result, registry::controller::ControllerRegistry, worker::protocol::WorkerEvent,
};

pub fn start_worker_event_watcher(
    registry: ControllerRegistry,
    mut worker_event_rx: mpsc::Receiver<WorkerEvent>,
) {
    tokio::spawn(async move {
        loop {
            let event = worker_event_rx
                .recv()
                .await
                .expect("Channel unexpectedly closed");

            // Handle different types of worker events
            if let Err(e) = handle_worker_event(&registry, event).await {
                error!("Failed to handle worker event: {}", e);
            }
        }
    });
}

async fn handle_worker_event(registry: &ControllerRegistry, event: WorkerEvent) -> Result<()> {
    match event {
        WorkerEvent::WorkerRegistration {
            worker_uid,
            worker_pid,
            worker_se_info,
        } => {
            // Send CGroupReparentCommand to system worker for this worker process
            if let Err(e) = registry.send_cgroup_reparent_command(worker_pid).await {
                error!(
                    "Failed to reparent worker {worker_uid:?} (SEInfo {worker_se_info}) PID {worker_pid}: {e}"
                );
            } else {
                info!(
                    "Successfully requested reparent for worker {worker_uid:?} (SEInfo {worker_se_info}) PID {worker_pid}"
                );
            }
        }
        WorkerEvent::Heartbeat { .. } => {
            // TODO: Do something?
            // info!(
            //     "Worker {worker_uid:?} heartbeat: uptime={uptime_seconds}s, active={active_services}",
            // );
        }
        WorkerEvent::ProcessSpawned {
            service_name,
            pinit_id,
            pid,
        } => {
            info!("Process spawned: {service_name} (ID: {pinit_id}) with PID {pid}");

            registry
                .update_service_state(service_name, ServiceRunState::Running { pid: Some(pid) })
                .await?;
        }
        WorkerEvent::ProcessExited {
            service_name,
            exit_code,
        } => {
            info!("Process exited: {} (code: {})", service_name, exit_code);

            registry
                .update_service_state(service_name, ServiceRunState::Stopped)
                .await?;
        }
        WorkerEvent::ProcessCrashed {
            service_name,
            signal,
        } => {
            error!("Process crashed: {service_name} (signal: {signal})");

            registry
                .update_service_state(
                    service_name,
                    ServiceRunState::Failed {
                        reason: format!("Process crashed with signal {}", signal),
                    },
                )
                .await?;
        }
        WorkerEvent::WorkerError {
            service_name,
            error,
        } => {
            error!("Worker error for {:?}: {}", service_name, error);

            // If there's a specific service, mark it as failed
            if let Some(service_name) = service_name {
                registry
                    .update_service_state(
                        service_name,
                        ServiceRunState::Failed {
                            reason: format!("Worker error: {}", error),
                        },
                    )
                    .await?;
            }
        }
    }

    Ok(())
}
