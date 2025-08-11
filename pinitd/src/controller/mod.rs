use std::{process, sync::Arc, time::Duration};

use pinitd_common::{
    CONTROL_SOCKET_ADDRESS, create_core_directories,
    protocol::{
        CLICommand, CLIResponse,
        writable::{ProtocolRead, ProtocolWrite},
    },
};
use pms::ProcessManagementService;
use tokio::{
    io::AsyncRead,
    net::TcpListener,
    signal::unix::{SignalKind, signal},
    sync::{Mutex, mpsc},
    task::JoinHandle,
    time::sleep,
};
use tokio_util::sync::CancellationToken;
use worker::start_worker_event_watcher;

use crate::{
    error::Result,
    exploit::{exploit, init_exploit, trigger_exploit_crash},
    file::acquire_controller_lock,
    registry::{Registry, controller::ControllerRegistry},
    worker::protocol::WorkerEvent,
    wrapper::daemonize,
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
    pub fn specialize(use_system_domain: bool, is_zygote: bool) -> Result<()> {
        let controller_lock = match acquire_controller_lock() {
            Some(lock) => lock,
            None => {
                error!("Controller lock is already owned. Dying");
                return Ok(());
            }
        };

        if is_zygote {
            init_zygote_with_fd();
        }

        info!("Pausing before demonization");
        std::thread::sleep(Duration::from_secs(2));

        daemonize(async move {
            // TODO: fnctl locks will be implicitly dropped when the original forker, the parent (grandparent) dies
            // There is not actually a lock held here
            let _ = controller_lock.unlock();
            init_exploit(use_system_domain).await?;

            info!("Sending exploit force clear");
            let _ = exploit()?.force_clear_exploit();

            create_core_directories();

            let (worker_event_tx, global_worker_event_rx) = mpsc::channel::<WorkerEvent>(100);

            let mut registry = ControllerRegistry::new(
                worker_event_tx,
                Arc::new(Mutex::new(Some(controller_lock))),
            )
            .await?;
            let pms = ProcessManagementService::new(registry.clone()).await?;
            registry.set_pms(pms).await;
            let mut controller = Controller { registry };

            controller.registry.load_from_disk().await?;
            let post_exploit = controller.registry.setup_workers().await?;

            let shutdown_token = CancellationToken::new();
            let shutdown_signal = setup_signal_watchers(shutdown_token.clone())?;

            start_worker_event_watcher(controller.registry.clone(), global_worker_event_rx);

            info!("Controller started");

            // Reparent controller process to system cgroup
            let controller_pid = std::process::id() as usize;
            info!("Reparenting controller process (PID {controller_pid}) to system cgroup",);
            if let Err(e) = controller
                .registry
                .send_cgroup_reparent_command(controller_pid)
                .await
            {
                error!("Failed to reparent controller process: {}", e);
            }

            let controller_clone = controller.clone();
            tokio::spawn(async move {
                let _ = controller_clone
                    .start_cli_listener(shutdown_token.clone())
                    .await;
            });

            info!("Autostarting services");
            controller
                .registry
                .queue_autostart_all(post_exploit)
                .await?;

            if !post_exploit {
                info!("Unlocking controller lock before crash");
                controller.registry.unlock_controller_file_lock().await;
                controller.registry.start_zygote_ready_watcher();

                sleep(Duration::from_millis(500)).await;

                info!("Sending Zygote crash");
                trigger_exploit_crash().await?;
                info!("Awaiting Zygote crash");
            }

            let _ = shutdown_signal.await;
            info!("Shutting down");

            controller.registry.unlock_controller_file_lock().await;
            shutdown(controller.registry).await?;

            info!("After shutdown");

            Ok(())
        })?
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

async fn shutdown(registry: ControllerRegistry) -> Result<()> {
    info!("Initiating daemon shutdown...");
    registry.shutdown().await?;

    info!("Goodbye");

    process::exit(0);
}
