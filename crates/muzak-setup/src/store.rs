//! What `muzak` remembers on this computer: the Spotify developer app, and each player's
//! Spotify sign-in (kept so a player can be set up again without signing in).
//! Lives in ~/.config/muzak (or $MUZAK_HOME).

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Saved {
    /// The Spotify developer app's Client ID; one app serves every player.
    #[serde(default)]
    pub spotify_client_id: Option<String>,
    /// The last Google Cloud key file used for voice, offered again next time.
    #[serde(default)]
    pub vertex_key_file: Option<PathBuf>,
}

pub fn home() -> PathBuf {
    if let Some(dir) = std::env::var_os("MUZAK_HOME") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    base.join("muzak")
}

fn private_dir(path: &PathBuf) -> anyhow::Result<()> {
    std::fs::create_dir_all(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub fn load() -> Saved {
    std::fs::read_to_string(home().join("muzak.toml"))
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(saved: &Saved) -> anyhow::Result<()> {
    let dir = home();
    private_dir(&dir)?;
    std::fs::write(dir.join("muzak.toml"), toml::to_string(saved)?)?;
    Ok(())
}

/// Where a player's Spotify sign-in is kept on this computer, by its address.
pub fn device_dir(host: &str) -> anyhow::Result<PathBuf> {
    let safe: String = host
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let dir = home().join("devices").join(safe);
    private_dir(&home())?;
    private_dir(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_choices_round_trip_in_a_private_folder() {
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: tests in this crate don't read MUZAK_HOME concurrently.
        unsafe { std::env::set_var("MUZAK_HOME", dir.path().join("m")) };
        save(&Saved {
            spotify_client_id: Some("abc".into()),
            vertex_key_file: None,
        })
        .unwrap();
        assert_eq!(load().spotify_client_id.as_deref(), Some("abc"));
        let device = device_dir("pi@muzak-den.local").unwrap();
        assert!(device.ends_with("devices/pi_muzak-den.local"));
        let mode = std::fs::metadata(&device).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}
