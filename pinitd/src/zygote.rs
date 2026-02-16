use std::{
    env,
    fs::File,
    io::Write,
    os::fd::{FromRawFd, RawFd},
    time::Duration,
};

use crate::error::{Error, Result};

const ZYGOTE_PID_WRITE_DELAY_MS: u64 = 1000;

pub fn init_zygote_with_fd() {
    #[cfg(target_os = "android")]
    {
        // Make sure the system has settled before removing the Zygote deadlock
        info!("Delaying {}ms before PID write", ZYGOTE_PID_WRITE_DELAY_MS);
        std::thread::sleep(Duration::from_millis(ZYGOTE_PID_WRITE_DELAY_MS));

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
    pipe.write_all(&helper_pid.to_be_bytes())?;
    pipe.flush()?;

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
