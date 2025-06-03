use std::{
    env,
    fs::File,
    io::Write,
    os::fd::{FromRawFd, RawFd},
    time::Duration,
};

use tokio::time::sleep;

use crate::error::{Error, Result};

pub async fn init_zygote_with_fd() {
    #[cfg(target_os = "android")]
    {
        info!("Delaying so Zygote can settle before pid write");
        sleep(Duration::from_millis(500)).await;
        if let Err(error) = extract_and_write_fd() {
            error!("fd error: {error}");
        }
    }
}

fn extract_and_write_fd() -> Result<()> {
    let fd_str = extract_fd().ok_or(Error::Unknown("Could not find fd".into()))?;
    let fd: RawFd = fd_str.parse::<RawFd>()?;

    let helper_pid = std::process::id() as u32;

    let mut pipe: File = unsafe { File::from_raw_fd(fd) };
    info!("Writing pid {helper_pid} to fd {fd}");
    let _ = pipe.write_all(&helper_pid.to_be_bytes());
    let _ = pipe.flush();

    Ok(())
}

fn extract_fd() -> Option<String> {
    let args = env::args().collect::<Vec<String>>();

    for i in 0..args.len() {
        let arg = &args[i];
        if arg == "com.android.internal.os.WrapperInit" {
            if i + 1 < args.len() {
                return Some(args[i + 1].clone());
            }
            return None;
        }
    }

    None
}
