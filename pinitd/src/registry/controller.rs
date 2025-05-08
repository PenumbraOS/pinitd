use pinitd_common::{ServiceRunState, ServiceStatus};

use crate::{
    error::{Error, Result},
    unit::{ServiceConfig, UID},
    worker::{
        connection::WorkerConnection,
        protocol::{WorkerCommand, WorkerResponse},
    },
};

use super::{Registry, local::LocalRegistry};

pub struct ControllerRegistry {
    local: LocalRegistry,
    remote: WorkerConnection,
}

impl Registry for ControllerRegistry {
    async fn service_names(&self) -> Result<Vec<String>> {
        self.local.service_names().await
    }

    async fn service_can_autostart(&self, name: String) -> Result<bool> {
        self.local.service_can_autostart(name).await
    }

    async fn insert_unit(&self, config: ServiceConfig, enabled: bool) -> Result<()> {
        self.local.insert_unit(config.clone(), enabled).await?;

        if config.uid == UID::System {
            self.remote
                .write_command(WorkerCommand::Create(config))
                .await?;
        }

        Ok(())
    }

    async fn remove_unit(&self, name: String) -> Result<bool> {
        let removed_local = self.local.remove_unit(name.clone()).await?;

        if removed_local && !self.local.is_shell_service(&name).await? {
            let response = self
                .remote
                .write_command(WorkerCommand::Destroy(name))
                .await?;
            Ok(response == WorkerResponse::Success)
        } else {
            Ok(removed_local)
        }
    }

    async fn service_start(&self, name: String) -> Result<bool> {
        let allow_start = self
            .local
            .with_service(&name, |service| {
                if !service.enabled() {
                    warn!("Attempted to start disabled service \"{name}\". Ignoring.",);
                    return Err(Error::Unknown(format!("Service \"{name}\" is disabled.")));
                }

                Ok(!matches!(service.state(), ServiceRunState::Running { .. }))
            })
            .await?;

        if !allow_start {
            // Already running
            return Ok(false);
        }

        if self.local.is_shell_service(&name).await? {
            self.local.service_start(name).await
        } else {
            let result = self
                .remote
                .write_command(WorkerCommand::Start(name))
                .await?;
            Ok(result == WorkerResponse::Success)
        }
    }

    async fn service_stop(&self, name: String) -> Result<()> {
        if self.local.is_shell_service(&name).await? {
            self.local.service_stop(name).await
        } else {
            self.remote.write_command(WorkerCommand::Stop(name)).await?;
            Ok(())
        }
    }

    async fn service_restart(&self, name: String) -> Result<()> {
        if self.local.is_shell_service(&name).await? {
            self.local.service_restart(name).await
        } else {
            self.remote
                .write_command(WorkerCommand::Restart(name))
                .await?;
            Ok(())
        }
    }

    async fn service_enable(&self, name: String) -> Result<()> {
        self.local.service_enable(name).await
    }

    async fn service_disable(&self, name: String) -> Result<()> {
        self.local.service_disable(name).await
    }

    async fn service_reload(&self, name: String) -> Result<Option<ServiceConfig>> {
        let result: Option<ServiceConfig> = self.local.service_reload(name.clone()).await?;
        if self.local.is_shell_service(&name).await? {
            if let Some(config) = &result {
                self.remote
                    .write_command(WorkerCommand::Create(config.clone()))
                    .await?;
            }
        }

        Ok(result)
    }

    async fn service_status(&self, name: String) -> Result<ServiceStatus> {
        self.local.service_status(name).await
    }

    async fn service_list_all(&self) -> Result<Vec<ServiceStatus>> {
        self.local.service_list_all().await
    }

    async fn shutdown(&self) -> Result<()> {
        self.remote.write_command(WorkerCommand::Shutdown).await?;
        Ok(())
    }
}
