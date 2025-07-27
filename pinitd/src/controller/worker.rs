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
        WorkerEvent::WorkerRegistration { worker_uid } => {
            info!("Worker {worker_uid:?} registered");
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
        } => {
            info!("Process spawned: {service_name} (ID: {pinit_id})");

            registry
                .update_service_state(service_name, ServiceRunState::Running { pid: None })
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
