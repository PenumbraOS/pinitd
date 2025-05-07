use pinitd_common::ServiceStatus;

use crate::{error::Error, unit::ServiceConfig};

pub mod controller;
pub mod local;

pub trait Registry {
    async fn service_names(&self) -> Result<Vec<String>, Error>;
    async fn service_can_autostart(&self, name: String) -> Result<bool, Error>;

    async fn insert_unit(&self, config: ServiceConfig, enabled: bool) -> Result<(), Error>;
    async fn remove_unit(&self, name: String) -> Result<bool, Error>;

    /// Attempts to start the registered service. Returns true if the service was successfully started and
    /// false if the service was already running
    async fn service_start(&self, name: String) -> Result<bool, Error>;
    async fn service_stop(&self, name: String) -> Result<(), Error>;
    async fn service_restart(&self, name: String) -> Result<(), Error>;

    async fn autostart_all(&self) -> Result<(), Error> {
        // Build current list of registry in case it's mutated during iteration and to drop lock
        let service_names = self.service_names().await?;

        for name in service_names {
            let should_start = self.service_can_autostart(name.clone()).await?;

            if !should_start {
                continue;
            }

            info!("Autostarting service: {}", name);
            let _ = self.service_start(name.clone()).await;
        }

        info!("Autostart sequence complete.");

        Ok(())
    }

    async fn service_enable(&self, name: String) -> Result<(), Error>;
    async fn service_disable(&self, name: String) -> Result<(), Error>;

    async fn service_reload(&self, name: String) -> Result<Option<ServiceConfig>, Error>;

    async fn service_status(&self, name: String) -> Result<ServiceStatus, Error>;
    async fn service_list_all(&self) -> Result<Vec<ServiceStatus>, Error>;

    async fn shutdown(&self) -> Result<(), Error>;
}
