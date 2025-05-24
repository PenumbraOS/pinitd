use std::process::Stdio;

use tokio::process::Command;

use crate::error::Result;

pub struct Wrapper;

impl Wrapper {
    pub async fn specialize(command: String) -> Result<()> {
        info!("Spawning child \"{command}\"");
        let mut child = Command::new("sh")
            .args(&["-c", &command])
            // TODO: Auto pipe output to Android log?
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Make sure we clean up if we die
            .kill_on_drop(true)
            .spawn()?;

        info!("Spawned process with pid {:?}", child.id());

        // TODO: Report status to controller
        let exit_status = child.wait().await?;
        info!("Process terminated with code {:?}", exit_status.code());

        Ok(())
    }
}
