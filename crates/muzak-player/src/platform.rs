//! Hardware bits: the DSI backlight, and restarting to apply settings. Bluetooth lives in
//! `crate::bluetooth`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use crate::app::DisplayMode;

pub struct Platform {
    backlight: Option<Arc<Backlight>>,
}

/// Dimming and going dark fade over this long; waking is instant.
const FADE: Duration = Duration::from_secs(5);
const FADE_STEPS: u32 = 25;

impl Platform {
    /// Must be called inside the tokio runtime.
    pub fn start() -> Self {
        let backlight = Backlight::detect();
        match &backlight {
            Some(b) => tracing::info!("backlight at {}", b.brightness.display()),
            None => tracing::info!("no backlight control; the UI overlay handles dimming"),
        }
        Self {
            backlight: backlight.map(Arc::new),
        }
    }

    pub fn set_display(&self, mode: DisplayMode) {
        let Some(backlight) = &self.backlight else {
            return;
        };
        let target = brightness_for(mode, backlight.max);
        // A newer change cancels a fade still running.
        let generation = backlight.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if mode == DisplayMode::Active {
            backlight.write(target);
            return;
        }
        let backlight = backlight.clone();
        tokio::spawn(async move {
            let from = backlight.current.load(Ordering::SeqCst);
            for step in 1..=FADE_STEPS {
                tokio::time::sleep(FADE / FADE_STEPS).await;
                if backlight.generation.load(Ordering::SeqCst) != generation {
                    return;
                }
                let value = from as i64
                    + (target as i64 - from as i64) * i64::from(step) / i64::from(FADE_STEPS);
                backlight.write(value as u32);
            }
        });
    }
}

pub fn brightness_for(mode: DisplayMode, max: u32) -> u32 {
    match mode {
        DisplayMode::Active => max,
        // 80% darker.
        DisplayMode::Dim => (max / 5).max(1),
        DisplayMode::Off => 0,
    }
}

struct Backlight {
    brightness: PathBuf,
    max: u32,
    /// The value last written.
    current: AtomicU32,
    /// Bumped by every display change, so an older fade stops.
    generation: AtomicU64,
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
            current: AtomicU32::new(max),
            generation: AtomicU64::new(0),
        })
    }

    fn write(&self, value: u32) {
        self.current.store(value, Ordering::SeqCst);
        if let Err(e) = std::fs::write(&self.brightness, value.to_string()) {
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
        assert_eq!(brightness_for(DisplayMode::Dim, 255), 51);
        assert_eq!(brightness_for(DisplayMode::Dim, 5), 1);
        assert_eq!(brightness_for(DisplayMode::Off, 255), 0);
    }
}
