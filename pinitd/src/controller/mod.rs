use std::{process, time::Duration};

use file_lock::{FileLock, FileOptions};
use pinitd_common::{
    CONTROL_SOCKET_ADDRESS, CONTROLLER_LOCK_FILE, create_core_directories,
    protocol::{
        CLICommand, CLIResponse,
        writable::{ProtocolRead, ProtocolWrite},
    },
};
use pms::ProcessManagementService;
use tokio::{
    io::AsyncRead,
    net::TcpListener,
    process::Command,
    signal::unix::{SignalKind, signal},
    sync::broadcast,
    task::JoinHandle,
    time::sleep,
};
use tokio_util::sync::CancellationToken;
use worker::{StartWorkerState, start_worker_update_watcher};

use crate::{
    error::Result,
    exploit::{exploit, init_exploit},
    registry::{Registry, controller::ControllerRegistry},
    worker::connection::WorkerConnectionStatus,
    zygote::init_zygote_with_fd,
};

pub mod pms;
mod worker;
mod zygote;

#[derive(Clone)]
pub struct Controller {
    registry: ControllerRegistry,
}

impl Controller {
    pub async fn specialize(
        use_system_domain: bool,
        disable_worker: bool,
        is_zygote: bool,
    ) -> Result<()> {
        // Acquire lock
        // TODO: We have to have a wrapped process so Zygote can't kill us
        let options = FileOptions::new().read(true).write(true).create(true);

        info!("Acquiring {CONTROLLER_LOCK_FILE}");
        let lock = match FileLock::lock(CONTROLLER_LOCK_FILE, false, options) {
            Ok(lock) => lock,
            Err(err) => {
                error!("Controller lock is already owned. Dying: {err}");
                return Ok(());
            }
        };
        info!("Acquired file lock");

        if is_zygote {
            init_zygote_with_fd().await;
        }

        info!("Delaying to allow Zygote to settle");
        sleep(Duration::from_millis(50)).await;

        init_exploit(use_system_domain).await?;

        info!("Sending exploit force clear");
        let _ = exploit()?.force_clear_exploit();

        create_core_directories();

        let StartWorkerState {
            connection,
            worker_service_update_rx,
            worker_connected_rx,
        } = StartWorkerState::start(disable_worker).await?;

        let mut registry =
            ControllerRegistry::new(connection, use_system_domain, disable_worker).await?;
        let pms = ProcessManagementService::new(registry.clone()).await?;
        registry.set_pms(pms).await;
        let mut controller = Controller { registry };

        controller.registry.load_from_disk().await?;

        let shutdown_token = CancellationToken::new();
        let shutdown_signal = setup_signal_watchers(shutdown_token.clone())?;

        start_worker_update_watcher(controller.registry.clone(), worker_service_update_rx);

        info!("Controller started");
        let controller_clone = controller.clone();
        tokio::spawn(async move {
            let _ = controller_clone
                .start_cli_listener(shutdown_token.clone())
                .await;
        });

        controller.start_worker_connection_listener(worker_connected_rx);

        info!("Autostarting services");
        controller.registry.autostart_all().await?;

        // TODO: Actually determine failure during crash scenarios while pinitd is still running
        sleep(Duration::from_secs(5)).await;
        mark_boot_success().await;

        let _ = shutdown_signal.await;
        info!("Shutting down");

        shutdown(controller.registry).await?;
        let _ = lock.unlock();

        info!("After shutdown");

        Ok(())
    }

    async fn start_cli_listener(&self, shutdown_token: CancellationToken) -> Result<()> {
        let control_socket = TcpListener::bind(&CONTROL_SOCKET_ADDRESS).await?;
        info!("Listening for CLI");

        loop {
            match control_socket.accept().await {
                Ok((mut stream, _)) => {
                    info!("Accepted new client connection");
                    let mut controller_clone = self.clone();
                    let shutdown_token_clone = shutdown_token.clone();
                    tokio::spawn(async move {
                        match controller_clone
                            .handle_command(&mut stream, shutdown_token_clone)
                            .await
                        {
                            Ok(response) => match response.write(&mut stream).await {
                                Ok(_) => {}
                                Err(err) => error!("Error responding to client: {err:?}"),
                            },
                            Err(err) => error!("Error handling client: {err:?}"),
                        }
                    });
                }
                Err(e) => {
                    error!("Error accepting control connection: {}", e);
                }
            }
        }
    }

    async fn handle_command<T>(
        &mut self,
        stream: &mut T,
        shutdown_token: CancellationToken,
    ) -> Result<CLIResponse>
    where
        T: AsyncRead + Unpin + Send,
    {
        let command = CLICommand::read(stream).await?;
        info!("Received CLICommand: {:?}", command);

        let response = self
            .registry
            .process_remote_command(command, shutdown_token)
            .await;

        Ok(response)
    }

    fn start_worker_connection_listener(
        &self,
        mut worker_connected_rx: broadcast::Receiver<WorkerConnectionStatus>,
    ) {
        let inner_controller = self.clone();
        tokio::spawn(async move {
            loop {
                let update = WorkerConnectionStatus::await_update(&mut worker_connected_rx).await;
                inner_controller
                    .clone()
                    .registry
                    .update_worker_connection(update)
                    .await;
            }
        });
    }
}

fn setup_signal_watchers(shutdown_token: CancellationToken) -> Result<JoinHandle<()>> {
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    let shutdown_signal_task = tokio::spawn(async move {
        tokio::select! {
            _ = sigterm.recv() => {
                info!("Received SIGTERM, initiating shutdown...");
            },
            _ = sigint.recv() => {
                 info!("Received SIGINT (Ctrl+C), initiating shutdown...");
            },
            _ = shutdown_token.cancelled() => {
                info!("Received shutdown command, initiating shutdown...");
            }
        }
    });

    Ok(shutdown_signal_task)
}

async fn mark_boot_success() {
    info!("Marking pinitd boot complete");
    if let Ok(mut child) = Command::new("setprop")
        .args(["pinitd.boot.status", "success"])
        .spawn()
    {
        let _ = child.wait().await;
    }
}

async fn shutdown(registry: ControllerRegistry) -> Result<()> {
    info!("Initiating daemon shutdown...");
    registry.shutdown().await?;

    info!("Goodbye");

    process::exit(0);
}
