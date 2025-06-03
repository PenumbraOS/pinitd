use std::{process::Stdio, time::Duration};

use pinitd_common::{
    PMS_SOCKET_ADDRESS,
    protocol::{
        PMSFromRemoteCommand, PMSToRemoteCommand,
        writable::{ProtocolRead, ProtocolWrite},
    },
};
use tokio::{
    net::TcpStream,
    process::{Child, Command},
    time::timeout,
};
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    zygote::init_zygote_with_fd,
};

pub struct Wrapper {
    stream: Option<TcpStream>,
}

impl Wrapper {
    pub async fn specialize_without_monitoring(
        command: String,
        using_zygote_spawn: bool,
    ) -> Result<Child> {
        if using_zygote_spawn {
            init_zygote_with_fd().await;
        }

        info!("Spawning child \"{command}\"");
        let child = Command::new("sh")
            .args(&["-c", &command])
            // TODO: Auto pipe output to Android log?
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        info!("Spawned process with pid {:?}", child.id());

        Ok(child)
    }

    pub async fn specialize_with_monitoring(
        command: String,
        pinit_id: Uuid,
        using_zygote_spawn: bool,
    ) -> Result<()> {
        info!("Negociating launch for id {pinit_id}");
        let stream = match TcpStream::connect(PMS_SOCKET_ADDRESS).await {
            Ok(mut stream) => {
                negoticate_launch(&mut stream, pinit_id).await?;
                Some(stream)
            }
            Err(_) => {
                warn!("Could not connect to PMS, continuing with spawn");
                None
            }
        };

        let mut wrapper = Wrapper { stream };

        let child = Self::specialize_without_monitoring(command, using_zygote_spawn).await?;

        if let Some(pid) = child.id() {
            let _ = wrapper
                .write_if_connected(PMSFromRemoteCommand::ProcessAttached(pid))
                .await;
        }

        // TODO: Handle subsequent commands
        let output = child.wait_with_output().await?;
        info!("Process terminated with code {:?}", output.status.code());

        if !output.status.success() {
            info!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        }

        let _ = wrapper
            .write_if_connected(PMSFromRemoteCommand::ProcessExited(output.status.code()))
            .await;

        Ok(())
    }

    async fn write_if_connected(&mut self, command: PMSFromRemoteCommand) -> Result<()> {
        if let Some(stream) = &mut self.stream {
            Ok(command.write(stream).await?)
        } else {
            Ok(())
        }
    }
}

async fn negoticate_launch(stream: &mut TcpStream, pinit_id: Uuid) -> Result<()> {
    timeout(Duration::from_secs(2), async move {
        PMSFromRemoteCommand::WrapperLaunched(pinit_id)
            .write(stream)
            .await?;

        info!("Waiting for controller response");
        let pms_response = PMSToRemoteCommand::read(stream).await?;
        info!("Controller response {pms_response:?}");

        match pms_response {
            PMSToRemoteCommand::Kill => {
                return Err(Error::ProcessSpawnError(
                    "PMS requested wrapper kill. Dying".to_string(),
                ));
            }
            _ => {}
        }

        Ok(())
    })
    .await?
}
