//! Hardware bits: the DSI backlight, and restarting to apply settings. Bluetooth lives in
//! `crate::bluetooth`.

use std::path::PathBuf;

use crate::app::DisplayMode;

pub struct Platform {
    backlight: Option<Backlight>,
}

impl Platform {
    /// Must be called inside the tokio runtime.
    pub fn start() -> Self {
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

/// Replaces this process with a fresh copy of itself, with the same arguments, so new
/// settings take effect. Under systemd the service keeps its PID; on a Mac the window reopens.
pub fn restart() -> ! {
    use std::os::unix::process::CommandExt;
    let error = match std::env::current_exe() {
        Ok(exe) => std::process::Command::new(exe)
            .args(std::env::args_os().skip(1))
            .exec(),
        Err(e) => e,
    };
    tracing::error!("restarting failed: {error}");
    // systemd restarts the service on exit; on a Mac the user starts it again.
    std::process::exit(1)
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
}
