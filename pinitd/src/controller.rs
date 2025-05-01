use std::{
    collections::HashMap, io::ErrorKind, path::PathBuf, sync::Arc, thread::sleep, time::Duration,
};

use android_31317_exploit::exploit::{ExploitKind, TriggerApp, build_and_execute};
use pinitd_common::{CONFIG_DIR, ServiceRunState};
use tokio::{fs, sync::Mutex};

use crate::{
    error::Error,
    process::spawn_and_monitor_service,
    socket::connect_worker,
    state::State,
    types::{Service, ServiceRegistry},
    unit::ServiceConfig,
};

pub struct Controller {}

impl Controller {
    pub async fn create() -> Result<(), Error> {
        let registry = Arc::new(Mutex::new(HashMap::new()));

        load_configs_and_state(registry.clone()).await?;

        // let mut worker_socket = match connect_worker() {
        //     Ok(socket) => Ok(socket),
        //     Err(error) => match error.kind() {
        //         ErrorKind::ConnectionRefused => {
        //             // Worker isn't alive, attempt to start
        //             warn!("Worker doesn't appear to be alive. Waiting 2s");
        //             sleep(Duration::from_secs(2));
        //             warn!("Attempting to start worker");
        //         }
        //         _ => Err(error),
        //     },
        // }?;

        // worker_socket.set_read_timeout(None)?;

        // DataFrame::new("Hello world".into()).send(&mut worker_socket)?;

        // worker_socket.shutdown(std::net::Shutdown::Both)?;
        warn!("Controller started");

        Ok(())
    }
}

async fn load_configs_and_state(registry: ServiceRegistry) -> Result<(), Error> {
    info!("Loading service configurations from {}", CONFIG_DIR);
    let _ = fs::create_dir_all(CONFIG_DIR).await;

    let state = State::load().await?;
    info!("Loaded enabled state for: {:?}", state.enabled_services);

    let mut directory = fs::read_dir(CONFIG_DIR).await?;

    let mut services = HashMap::new();

    while let Some(entry) = directory.next_entry().await? {
        let path = entry.path();
        if path.is_file() && path.extension().map_or(false, |ext| ext == "unit") {
            match ServiceConfig::parse(&path).await {
                Ok(config) => {
                    let name = config.name.clone();
                    let enabled = state.enabled_services.contains(&name);
                    info!("Loaded config: {} (Enabled: {})", name, enabled);

                    let runtime = Service {
                        config,
                        state: ServiceRunState::Stopped,
                        enabled,
                        monitor_task: None,
                    };
                    services.insert(name, runtime);
                }
                Err(e) => {
                    warn!("Failed to parse unit file {:?}: {}", path, e);
                }
            }
        }
    }

    // TODO: Look at this
    let mut reg = registry.lock().await;
    *reg = services;
    info!(
        "Finished loading configurations. {} services loaded.",
        reg.len()
    );
    Ok(())
}

async fn autostart(registry: ServiceRegistry) {
    let registry_lock = registry.lock().await;

    // Build current list of registry in case it's mutated during iteration and to drop lock
    let service_names = registry_lock.keys().cloned().collect::<Vec<String>>();

    for name in service_names {
        // Each iteration of the loop reacquires a lock
        let registry_lock = registry.lock().await;

        let should_start = {
            let service = registry_lock.get(&name).unwrap();
            service.enabled && service.config.autostart && service.state == ServiceRunState::Stopped
        };

        if !should_start {
            continue;
        }

        info!("Autostarting service: {}", name);
        // Drop lock in case it's required by spawn
        drop(registry_lock);
        // Pass the Arc, spawn_and_monitor will lock when needed
        let _ = spawn_and_monitor_service(name.clone(), registry.clone()).await;
    }

    info!("Autostart sequence complete.");
}
