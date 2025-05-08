use pinitd_common::{
    ServiceStatus,
    protocol::{CLICommand, CLIResponse},
};
use tokio_util::sync::CancellationToken;

use crate::{error::Result, unit::ServiceConfig};

pub mod controller;
pub mod local;

pub trait Registry {
    async fn service_names(&self) -> Result<Vec<String>>;
    async fn service_can_autostart(&self, name: String) -> Result<bool>;

    async fn insert_unit(&self, config: ServiceConfig, enabled: bool) -> Result<()>;
    async fn remove_unit(&self, name: String) -> Result<bool>;

    /// Attempts to start the registered service. Returns true if the service was successfully started and
    /// false if the service was already running
    async fn service_start(&self, name: String) -> Result<bool>;
    async fn service_stop(&self, name: String) -> Result<()>;
    async fn service_restart(&self, name: String) -> Result<()>;

    async fn autostart_all(&self) -> Result<()> {
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

    async fn service_enable(&self, name: String) -> Result<()>;
    async fn service_disable(&self, name: String) -> Result<()>;

    async fn service_reload(&self, name: String) -> Result<Option<ServiceConfig>>;

    async fn service_status(&self, name: String) -> Result<ServiceStatus>;
    async fn service_list_all(&self) -> Result<Vec<ServiceStatus>>;

    async fn shutdown(&self) -> Result<()>;

    async fn process_remote_command(
        &self,
        command: CLICommand,
        shutdown_token: CancellationToken,
    ) -> CLIResponse {
        match command {
            CLICommand::Start(name) => match self.service_start(name.clone()).await {
                Ok(did_start) => {
                    if did_start {
                        CLIResponse::Success(format!("Service \"{name}\" started",))
                    } else {
                        CLIResponse::Success(format!("Service \"{name}\" already running",))
                    }
                }
                Err(err) => {
                    CLIResponse::Error(format!("Failed to start service \"{name}\": {err}"))
                }
            },
            CLICommand::Stop(name) => match self.service_stop(name.clone()).await {
                Ok(_) => CLIResponse::Success(format!("Service \"{name}\" stop initiated.")),
                Err(err) => CLIResponse::Error(format!("Failed to stop service \"{name}\": {err}")),
            },
            CLICommand::Restart(name) => match self.service_restart(name.clone()).await {
                Ok(_) => CLIResponse::Success(format!("Service \"{name}\" restarted")),
                Err(err) => {
                    CLIResponse::Error(format!("Failed to restart service \"{name}\": {err}"))
                }
            },
            CLICommand::Enable(name) => match self.service_enable(name.clone()).await {
                Ok(_) => CLIResponse::Success(format!("Service \"{name}\" enabled")),
                Err(err) => {
                    CLIResponse::Error(format!("Failed to enable service \"{name}\": {err}"))
                }
            },
            CLICommand::Disable(name) => match self.service_disable(name.clone()).await {
                Ok(_) => CLIResponse::Success(format!("Service \"{name}\" disabled")),
                Err(err) => {
                    CLIResponse::Error(format!("Failed to disable service \"{name}\": {err}"))
                }
            },
            CLICommand::Reload(name) => match self.service_reload(name.clone()).await {
                Ok(_) => CLIResponse::Success(format!("Service \"{name}\" reloaded")),
                Err(err) => {
                    CLIResponse::Error(format!("Failed to reload service \"{name}\": {err}"))
                }
            },
            CLICommand::Status(name) => match self.service_status(name).await {
                Ok(status) => CLIResponse::Status(status),
                Err(err) => CLIResponse::Error(err.to_string()),
            },
            CLICommand::List => match self.service_list_all().await {
                Ok(list) => CLIResponse::List(list),
                Err(err) => CLIResponse::Error(format!("Failed to retrieve service list: {err}")),
            },
            CLICommand::Shutdown => {
                info!("Shutdown RemoteCommand received.");
                shutdown_token.cancel();
                CLIResponse::ShuttingDown // Respond immediately
            }
        }
    }
}
