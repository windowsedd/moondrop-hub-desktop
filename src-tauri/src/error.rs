use serde::Serialize;
use thiserror::Error;

/// Structured error type for Moondrop USB-HID operations.
#[derive(Debug, Error)]
pub enum HidError {
    #[error("USB-HID subsystem error: {0}")]
    HidApi(#[from] hidapi::HidError),

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Device is not currently open: {0}")]
    DeviceNotOpen(String),

    #[error("Internal state lock poisoned")]
    StateLockPoisoned,

    #[error("Invalid tray icon data: {0}")]
    InvalidIcon(String),
}

impl Serialize for HidError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
