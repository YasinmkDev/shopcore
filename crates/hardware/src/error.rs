//! Hardware errors. A device failure is never fatal to a sale — the
//! caller queues the action (print, drawer kick) and the sale completes
//! (see `context/06-hardware-integrations.md`).

use thiserror::Error;

/// What can go wrong talking to a device.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HardwareError {
    /// The device is not connected / not reachable.
    #[error("device offline: {0}")]
    Offline(String),
    /// The device did not respond within its timeout.
    #[error("device timed out: {0}")]
    Timeout(String),
    /// The driver's scan queue is full (mock only — the caller is
    /// feeding scans faster than the worker drains them).
    #[error("scan queue full")]
    QueueFull,
    /// A driver-specific failure.
    #[error("driver error: {0}")]
    Driver(String),
}
