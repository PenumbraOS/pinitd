use std::{
    env,
    fs::File,
    io::Write,
    os::fd::{FromRawFd, RawFd},
};

use crate::error::Error;

pub fn extract_and_write_fd() -> Result<(), Error> {
    let fd_str = extract_fd().ok_or(Error::Unknown("Could not find fd".into()))?;
    let fd: RawFd = fd_str.parse::<RawFd>()?;

    let helper_pid = std::process::id() as u32;

    let mut pipe: File = unsafe { File::from_raw_fd(fd) };
    warn!("Opening fd {fd}, {pipe:?}");
    pipe.write_all(&helper_pid.to_be_bytes()).unwrap();
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
