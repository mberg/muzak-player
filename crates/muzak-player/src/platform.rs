//! Hardware bits: DSI backlight and a Bluetooth speaker. Both are no-ops on the Mac.

use std::path::PathBuf;
use std::time::Duration;

use tokio::process::Command;
use tokio::sync::mpsc::UnboundedSender;

use crate::app::{DisplayMode, Input};
use crate::config::Config;

pub struct Platform {
    backlight: Option<Backlight>,
}

impl Platform {
    /// Must be called inside the tokio runtime.
    pub fn start(config: &Config, inputs: UnboundedSender<Input>) -> Self {
        if let Some(address) = config.bluetooth_speaker.clone() {
            tokio::spawn(monitor_speaker(address, inputs));
        }
        let backlight = Backlight::detect();
        match &backlight {
            Some(b) => tracing::info!("backlight at {}", b.brightness.display()),
            None => tracing::info!("no backlight control; the UI overlay handles dimming"),
        }
        Self { backlight }
    }

    pub fn set_display(&self, mode: DisplayMode) {
        if let Some(backlight) = &self.backlight {
            backlight.set(mode);
        }
    }
}

pub fn brightness_for(mode: DisplayMode, max: u32) -> u32 {
    match mode {
        DisplayMode::Active => max,
        DisplayMode::Dim => (max * 15 / 100).max(1),
        DisplayMode::Off => 0,
    }
}

struct Backlight {
    brightness: PathBuf,
    max: u32,
}

impl Backlight {
    fn detect() -> Option<Self> {
        let dir = std::fs::read_dir("/sys/class/backlight")
            .ok()?
            .flatten()
            .next()?
            .path();
        let max = std::fs::read_to_string(dir.join("max_brightness"))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        Some(Self {
            brightness: dir.join("brightness"),
            max,
        })
    }

    fn set(&self, mode: DisplayMode) {
        let value = brightness_for(mode, self.max).to_string();
        if let Err(e) = std::fs::write(&self.brightness, value) {
            tracing::warn!("setting backlight failed: {e}");
        }
    }
}

pub fn parse_connected(bluetoothctl_info: &str) -> bool {
    bluetoothctl_info
        .lines()
        .any(|line| line.trim() == "Connected: yes")
}

async fn bluetoothctl(args: &[&str]) -> Option<String> {
    let run = Command::new("bluetoothctl")
        .args(args)
        .kill_on_drop(true)
        .output();
    match tokio::time::timeout(Duration::from_secs(10), run).await {
        Ok(Ok(output)) => Some(String::from_utf8_lossy(&output.stdout).into_owned()),
        _ => None,
    }
}

async fn monitor_speaker(address: String, inputs: UnboundedSender<Input>) {
    let mut last = None;
    loop {
        let mut connected = bluetoothctl(&["info", &address])
            .await
            .is_some_and(|out| parse_connected(&out));
        if !connected {
            let _ = bluetoothctl(&["connect", &address]).await;
            connected = bluetoothctl(&["info", &address])
                .await
                .is_some_and(|out| parse_connected(&out));
        }
        if last != Some(connected) {
            tracing::info!("speaker {address} connected: {connected}");
            let _ = inputs.send(Input::Speaker { connected });
            last = Some(connected);
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_levels() {
        assert_eq!(brightness_for(DisplayMode::Active, 255), 255);
        assert_eq!(brightness_for(DisplayMode::Dim, 255), 38);
        assert_eq!(brightness_for(DisplayMode::Dim, 5), 1);
        assert_eq!(brightness_for(DisplayMode::Off, 255), 0);
    }

    #[test]
    fn parses_bluetoothctl_info() {
        let out =
            "Device AA:BB:CC:DD:EE:FF (public)\n\tName: Speaker\n\tPaired: yes\n\tConnected: yes\n";
        assert!(parse_connected(out));
        assert!(!parse_connected(
            &out.replace("Connected: yes", "Connected: no")
        ));
        assert!(!parse_connected(""));
    }
}
