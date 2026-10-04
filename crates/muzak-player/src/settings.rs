//! Choices made on the device's Settings screen, kept in `<state_dir>/settings.json`.
//! They override the config file, which stays as the default.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::Config;

pub const FILE_NAME: &str = "settings.json";
const JACK_DEVICE: &str = "plughw:CARD=Headphones";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Speaker {
    pub address: String,
    pub name: String,
}

/// A Sonos room on the home network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SonosRoom {
    /// Sonos's player id, e.g. "RINCON_000E58...".
    pub uuid: String,
    pub name: String,
}

/// Where audio goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Output {
    Jack,
    Bluetooth(Speaker),
    /// A Sonos room plays instead of this device, controlled over the network.
    Sonos(SonosRoom),
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Spotify Connect name; the config's `device_name` when unset.
    #[serde(default)]
    pub device_name: Option<String>,
    /// The config's audio settings when unset.
    #[serde(default)]
    pub output: Option<Output>,
    /// How long the sleep timer runs; 30 minutes when unset.
    #[serde(default)]
    pub sleep_minutes: Option<u32>,
    /// Colour scheme: 0 Midnight (default), 1 Ocean, 2 Forest, 3 Daylight.
    #[serde(default)]
    pub theme: Option<u32>,
}

impl Settings {
    /// A missing or unreadable file means no overrides.
    pub fn load(state_dir: &Path) -> Settings {
        let path = state_dir.join(FILE_NAME);
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                tracing::warn!("ignoring unreadable {}: {e}", path.display());
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self, state_dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(state_dir)?;
        std::fs::write(state_dir.join(FILE_NAME), serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    /// Layers these choices over the config file.
    pub fn apply(&self, config: &mut Config) {
        if let Some(name) = self.device_name.as_ref().filter(|n| !n.trim().is_empty()) {
            config.device_name = name.clone();
        }
        let alsa = config.audio_backend == "alsa";
        match &self.output {
            None => {}
            Some(Output::Jack) => {
                config.bluetooth_speaker = None;
                if alsa
                    && config
                        .audio_device
                        .as_deref()
                        .is_none_or(|d| d.starts_with("bluealsa"))
                {
                    config.audio_device = Some(JACK_DEVICE.into());
                }
            }
            // This device plays nothing; its own audio settings don't matter.
            Some(Output::Sonos(_)) => {}
            Some(Output::Bluetooth(speaker)) => {
                config.bluetooth_speaker = Some(speaker.address.clone());
                if alsa {
                    config.audio_device =
                        Some(format!("bluealsa:DEV={},PROFILE=a2dp", speaker.address));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pi_config() -> Config {
        Config::parse(
            "device_name = \"Muzak\"\nstate_dir = \"/tmp/x\"\naudio_backend = \"alsa\"\naudio_device = \"plughw:CARD=Headphones\"\n",
        )
        .unwrap()
    }

    #[test]
    fn missing_file_means_no_overrides() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
    }

    #[test]
    fn saved_settings_load_back() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            device_name: Some("Kitchen".into()),
            output: Some(Output::Jack),
            sleep_minutes: Some(60),
            theme: Some(2),
        };
        settings.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()), settings);
    }

    #[test]
    fn bluetooth_speaker_routes_audio_through_bluealsa() {
        let mut config = pi_config();
        Settings {
            device_name: Some("Kitchen".into()),
            output: Some(Output::Bluetooth(Speaker {
                address: "AA:BB:CC:DD:EE:FF".into(),
                name: "Boom".into(),
            })),
            sleep_minutes: None,
            theme: None,
        }
        .apply(&mut config);
        assert_eq!(config.device_name, "Kitchen");
        assert_eq!(
            config.audio_device.as_deref(),
            Some("bluealsa:DEV=AA:BB:CC:DD:EE:FF,PROFILE=a2dp")
        );
        assert_eq!(
            config.bluetooth_speaker.as_deref(),
            Some("AA:BB:CC:DD:EE:FF")
        );
    }

    #[test]
    fn jack_undoes_a_configured_speaker() {
        let mut config = pi_config();
        config.audio_device = Some("bluealsa:DEV=AA:BB:CC:DD:EE:FF,PROFILE=a2dp".into());
        config.bluetooth_speaker = Some("AA:BB:CC:DD:EE:FF".into());
        Settings {
            device_name: None,
            output: Some(Output::Jack),
            sleep_minutes: None,
            theme: None,
        }
        .apply(&mut config);
        assert_eq!(config.audio_device.as_deref(), Some(JACK_DEVICE));
        assert_eq!(config.bluetooth_speaker, None);
        assert_eq!(config.device_name, "Muzak");
    }
}
