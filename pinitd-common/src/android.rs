use crate::error::{Error, Result};
use std::{process::Stdio, time::Duration};
use tokio::{process::Command, time::timeout};

pub async fn fetch_package_path(package: &str) -> Result<String> {
    let child = Command::new("pm")
        .args(&["path", package])
        // TODO: Auto pipe output to Android log?
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let output = match timeout(Duration::from_millis(500), child.wait_with_output()).await {
        Ok(Ok(output)) => {
            if !output.status.success() {
                Err(Error::PackageLookup(format!(
                    "Could not find package {package}"
                )))
            } else {
                Ok(output)
            }
        }
        Ok(Err(_)) => Err(Error::PackageLookup(format!(
            "Could not find package {package}"
        ))),
        Err(_) => Err(Error::PackageLookup(format!(
            "Could not find package {package}"
        ))),
    }?;

    let stdout = String::from_utf8(output.stdout).ok();

    if let Some(stdout) = stdout {
        let package_path = stdout.trim_start_matches("package:").trim();
        if !package_path.starts_with("/data/app") {
            return Err(Error::PackageLookup(format!(
                "Found invalid package path for package {package}. Found {package_path}"
            )));
        }

        return Ok(package_path.into());
    }

    Err(Error::PackageLookup(format!(
        "Could not find package {package}"
    )))
}
