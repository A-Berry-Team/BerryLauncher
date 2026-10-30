//! Small probes of the host machine that several crates need.

use std::fs::OpenOptions;

/// State of the `/dev/ntsync` device as seen by the launcher.
pub enum NtsyncDevice {
    Usable,
    Missing,
    Denied(String),
}

pub fn probe_ntsync_device() -> NtsyncDevice {
    match OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/ntsync")
    {
        Ok(_) => NtsyncDevice::Usable,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => NtsyncDevice::Missing,
        Err(error) => NtsyncDevice::Denied(error.to_string()),
    }
}

/// Is `/dev/ntsync` present and accessible to the launcher?
pub fn ntsync_available() -> bool {
    matches!(probe_ntsync_device(), NtsyncDevice::Usable)
}
