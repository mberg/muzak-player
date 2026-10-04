use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};
use serde::Deserialize;

/// Device configuration, read from `/etc/muzak/config.toml` on the Pi.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Shown as the Spotify Connect device name.
    pub device_name: String,
    /// librespot audio backend: "alsa" on the Pi, "rodio" on the Mac.
    #[serde(default = "default_audio_backend")]
    pub audio_backend: String,
    /// ALSA device, e.g. "plughw:CARD=Headphones" or "bluealsa:DEV=AA:BB:CC:DD:EE:FF,PROFILE=a2dp".
    #[serde(default)]
    pub audio_device: Option<String>,
    pub state_dir: PathBuf,
    #[serde(default = "default_dim_after_secs")]
    pub dim_after_secs: u64,
    #[serde(default = "default_off_after_secs")]
    pub off_after_secs: u64,
    #[serde(default = "default_initial_volume")]
    pub initial_volume: u8,
    /// Bluetooth speaker address to watch and reconnect.
    #[serde(default)]
    pub bluetooth_speaker: Option<String>,
    /// Audiobookshelf server, e.g. "http://nas.local:13378". Turns Books on.
    #[serde(default)]
    pub audiobookshelf_url: Option<String>,
    /// Voice control: a sherpa-onnx keyword model folder. Voice is on when this is set.
    #[serde(default)]
    pub voice_model_dir: Option<PathBuf>,
    /// Said before a request; defaults to "ziggy".
    #[serde(default)]
    pub wake_phrase: Option<String>,
    /// How sure the wake word must be, 0-1; lower hears it more readily. Defaults to 0.25.
    #[serde(default)]
    pub wake_threshold: Option<f32>,
    /// Part of the microphone's name; the default input when unset.
    #[serde(default)]
    pub microphone: Option<String>,
    /// Gemini API key for voice requests; GEMINI_API_KEY in the environment also works.
    #[serde(default)]
    pub gemini_api_key: Option<String>,
    /// A Google Cloud service account key file: voice requests go to Vertex AI instead,
    /// paid from the project's billing (and its credits).
    #[serde(default)]
    pub vertex_key_file: Option<PathBuf>,
    /// Vertex AI location; defaults to "global".
    #[serde(default)]
    pub vertex_location: Option<String>,
    /// Defaults to gemini-3.5-flash-lite.
    #[serde(default)]
    pub gemini_model: Option<String>,
}

fn default_audio_backend() -> String {
    if cfg!(target_os = "linux") {
        "alsa"
    } else {
        "rodio"
    }
    .to_string()
}
fn default_dim_after_secs() -> u64 {
    180
}
fn default_off_after_secs() -> u64 {
    600
}
fn default_initial_volume() -> u8 {
    50
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Config> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing config {}", path.display()))
    }

    pub fn parse(text: &str) -> anyhow::Result<Config> {
        let config: Config = toml::from_str(text)?;
        ensure!(
            !config.device_name.trim().is_empty(),
            "device_name must not be blank"
        );
        ensure!(config.initial_volume <= 100, "initial_volume must be 0-100");
        ensure!(
            config.off_after_secs > config.dim_after_secs,
            "off_after_secs must be greater than dim_after_secs"
        );
        Ok(config)
    }

    pub fn librespot_dir(&self) -> PathBuf {
        self.state_dir.join("librespot")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.state_dir.join("cache")
    }

    pub fn images_dir(&self) -> PathBuf {
        self.state_dir.join("images")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_config_uses_defaults() {
        let c = Config::parse("device_name = \"Leo\"\nstate_dir = \"/var/lib/muzak\"\n").unwrap();
        assert_eq!(c.device_name, "Leo");
        assert_eq!(c.initial_volume, 50);
        assert_eq!(c.dim_after_secs, 180);
        assert_eq!(c.off_after_secs, 600);
        assert_eq!(c.audio_device, None);
        assert_eq!(c.bluetooth_speaker, None);
        assert_eq!(c.librespot_dir(), PathBuf::from("/var/lib/muzak/librespot"));
        assert_eq!(c.cache_dir(), PathBuf::from("/var/lib/muzak/cache"));
        assert_eq!(c.images_dir(), PathBuf::from("/var/lib/muzak/images"));
    }

    #[test]
    fn rejects_volume_over_100() {
        let err =
            Config::parse("device_name = \"Leo\"\nstate_dir = \"/x\"\ninitial_volume = 101\n")
                .unwrap_err();
        assert!(err.to_string().contains("initial_volume"), "{err}");
    }

    #[test]
    fn rejects_off_before_dim() {
        let err = Config::parse("device_name = \"Leo\"\nstate_dir = \"/x\"\ndim_after_secs = 600\noff_after_secs = 600\n").unwrap_err();
        assert!(err.to_string().contains("off_after_secs"), "{err}");
    }

    #[test]
    fn rejects_blank_name_and_unknown_keys() {
        assert!(Config::parse("device_name = \" \"\nstate_dir = \"/x\"\n").is_err());
        assert!(Config::parse("device_name = \"Leo\"\nstate_dir = \"/x\"\nvolume = 3\n").is_err());
    }

    #[test]
    fn example_device_config_parses() {
        let text = include_str!("../../../devices/example.toml");
        let c = Config::parse(text).unwrap();
        assert_eq!(c.audio_device.as_deref(), Some("plughw:CARD=Headphones"));
    }
}
