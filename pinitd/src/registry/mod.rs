use pinitd_common::{ServiceStatus, UID, unit_config::ServiceConfig};
use uuid::Uuid;

use crate::error::Result;

pub mod controller;
pub mod local;
pub mod spawn;

pub trait Registry {
    async fn service_names(&self) -> Result<Vec<String>>;
    async fn service_can_autostart(&self, name: String) -> Result<bool>;

    async fn insert_unit(&mut self, config: ServiceConfig, enabled: bool) -> Result<()>;
    async fn remove_unit(&mut self, name: String) -> Result<bool>;

    /// Attempts to start the registered service. Returns true if the service was successfully started and
    /// false if the service was already running
    async fn service_start_with_id(
        &mut self,
        name: String,
        id: Uuid,
        wait_for_start: bool,
    ) -> Result<bool>;

    async fn service_enable(&self, name: String) -> Result<()>;
    async fn service_disable(&self, name: String) -> Result<()>;

    async fn service_status(&self, name: String) -> Result<ServiceStatus>;
    async fn service_list_all(&self) -> Result<Vec<ServiceStatus>>;

    async fn shutdown(&self) -> Result<()>;

    /// Returns the UID for local "standard" (non-Zygote) spawns. Either `UID::System` or `UID::Shell`
    fn local_service_uid(&self) -> UID;

    /// Returns the UID for worker spawns. Either `UID::System` or `UID::Shell`
    fn worker_service_uid(&self) -> UID;
}
