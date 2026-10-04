//! Bluetooth speakers: scan, pair and connect, forget, and keep the chosen speaker connected.
//! Real Bluetooth runs only on the Pi (BlueZ over D-Bus); `--fake` mode has pretend speakers.

#[cfg(target_os = "linux")]
mod bluez;
mod fake;

use tokio::sync::mpsc::{self, UnboundedSender};

use crate::app::{BtCommand, Input};

/// How long a scan for speakers runs.
pub const SCAN_SECS: u64 = 15;

pub enum Backend {
    Fake,
    /// BlueZ on the Pi. `watch` is the chosen speaker's address, kept connected.
    Real {
        watch: Option<String>,
    },
}

/// Starts the Bluetooth task. Returns None where Bluetooth isn't available (a Mac in real
/// mode); the Settings screen then says so.
pub fn spawn(
    backend: Backend,
    inputs: UnboundedSender<Input>,
) -> Option<UnboundedSender<BtCommand>> {
    let (tx, rx) = mpsc::unbounded_channel();
    match backend {
        Backend::Fake => {
            tokio::spawn(fake::run(rx, inputs));
            Some(tx)
        }
        #[cfg(target_os = "linux")]
        Backend::Real { watch } => {
            tokio::spawn(bluez::run(watch, rx, inputs));
            Some(tx)
        }
        #[cfg(not(target_os = "linux"))]
        Backend::Real { .. } => None,
    }
}
