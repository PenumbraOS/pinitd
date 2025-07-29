use file_lock::{FileLock, FileOptions};
use pinitd_common::CONTROLLER_LOCK_FILE;

pub fn acquire_controller_lock() -> Option<FileLock> {
    info!("Acquiring {CONTROLLER_LOCK_FILE}");
    let options = FileOptions::new().read(true).write(true).create(true);

    let lock = match FileLock::lock(CONTROLLER_LOCK_FILE, false, options) {
        Ok(lock) => lock,
        Err(err) => {
            error!("Controller lock is already owned. Dying: {err}");
            return None;
        }
    };
    info!("Acquired file lock");
    Some(lock)
}
