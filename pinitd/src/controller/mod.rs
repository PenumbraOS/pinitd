use std::{process, time::Duration};

use file_lock::{FileLock, FileOptions};
use pinitd_common::{
    BOOT_SUCCESS_FILE, CONTROL_SOCKET_ADDRESS, CONTROLLER_LOCK_FILE, create_core_directories,
    protocol::{
        CLICommand, CLIResponse,
        writable::{ProtocolRead, ProtocolWrite},
    },
};
use pms::ProcessManagementService;
use tokio::{
    fs::File,
    io::AsyncRead,
    net::TcpListener,
    signal::unix::{SignalKind, signal},
    sync::mpsc,
    task::JoinHandle,
    time::sleep,
};
use tokio_util::sync::CancellationToken;
use worker::start_worker_event_watcher;

use crate::{
    error::Result,
    exploit::{exploit, init_exploit},
    registry::{Registry, controller::ControllerRegistry},
    worker::protocol::WorkerEvent,
    zygote::init_zygote_with_fd,
};

pub mod pms;
mod worker;
pub mod worker_manager;
mod zygote;

#[derive(Clone)]
pub struct Controller {
    registry: ControllerRegistry,
}

impl Controller {
    pub async fn specialize(use_system_domain: bool, is_zygote: bool) -> Result<()> {
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

        let (worker_event_tx, global_worker_event_rx) = mpsc::channel::<WorkerEvent>(100);

        let mut registry = ControllerRegistry::new(worker_event_tx).await?;
        let pms = ProcessManagementService::new(registry.clone()).await?;
        registry.set_pms(pms).await;
        let mut controller = Controller { registry };

        controller.registry.load_from_disk().await?;

        let shutdown_token = CancellationToken::new();
        let shutdown_signal = setup_signal_watchers(shutdown_token.clone())?;

        start_worker_event_watcher(controller.registry.clone(), global_worker_event_rx);

        info!("Controller started");
        let controller_clone = controller.clone();
        tokio::spawn(async move {
            let _ = controller_clone
                .start_cli_listener(shutdown_token.clone())
                .await;
        });

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

    // Create success file for Android app to detect
    match File::create(BOOT_SUCCESS_FILE).await {
        Ok(_) => {
            info!("Marked boot as success");
        }
        Err(err) => {
            error!("Failed to create boot success file: {}", err);
        }
    }
}

async fn shutdown(registry: ControllerRegistry) -> Result<()> {
    info!("Initiating daemon shutdown...");
    registry.shutdown().await?;

    info!("Goodbye");

    process::exit(0);
}
