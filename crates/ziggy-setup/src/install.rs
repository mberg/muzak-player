//! Putting the player on a Pi: system setup, the player itself, models, config and sign-ins.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::devconfig::{self, CONFIG_PATH, VERTEX_KEY_PATH, WAKE_MODEL};
use crate::remote::Remote;

pub const REPO: &str = "mberg/ziggy";
pub const PLAYER_ASSET: &str = "ziggy-player-linux-arm64.tar.gz";
const PROVISION: &str = include_str!("../../../deploy/provision-remote.sh");
const SERVICE: &str = include_str!("../../../deploy/ziggy-player.service");
const WAKE_MODEL_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models";

/// Where the player binary comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerSource {
    /// A GitHub release: the latest, or a tag such as "v0.2.0".
    Release(Option<String>),
    /// A binary built on this computer (scripts/build-pi.sh).
    Local(PathBuf),
}

pub fn release_url(tag: Option<&str>) -> String {
    match tag {
        Some(tag) => format!("https://github.com/{REPO}/releases/download/{tag}/{PLAYER_ASSET}"),
        None => format!("https://github.com/{REPO}/releases/latest/download/{PLAYER_ASSET}"),
    }
}

/// Installs packages, the ziggy-player user, the service, and display settings. Safe to run again.
/// Returns true when the Pi must reboot for display settings to apply.
pub fn provision(r: &dyn Remote) -> anyhow::Result<bool> {
    r.upload(
        PROVISION.as_bytes(),
        "/tmp/ziggy-provision/provision-remote.sh",
        "755",
        None,
    )?;
    r.upload(
        SERVICE.as_bytes(),
        "/tmp/ziggy-provision/ziggy-player.service",
        "644",
        None,
    )?;
    let out = r
        .run("sudo bash /tmp/ziggy-provision/provision-remote.sh; rc=$?; sudo rm -rf /tmp/ziggy-provision; exit $rc")
        .context("preparing the Pi")?;
    Ok(out.contains("ZIGGY_REBOOT_NEEDED"))
}

pub fn install_player(r: &dyn Remote, source: &PlayerSource) -> anyhow::Result<()> {
    match source {
        PlayerSource::Release(tag) => {
            let url = release_url(tag.as_deref());
            r.run(&format!(
                "tmp=$(mktemp -d)\n\
                 curl -fsSL '{url}' -o \"$tmp/player.tar.gz\"\n\
                 tar xzf \"$tmp/player.tar.gz\" -C \"$tmp\"\n\
                 sudo install -m 755 \"$tmp/ziggy-player\" /usr/local/bin/ziggy-player\n\
                 rm -rf \"$tmp\""
            ))
            .with_context(|| format!("downloading the player from {url}"))?;
        }
        PlayerSource::Local(path) => {
            let bytes =
                std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            r.upload(&bytes, "/usr/local/bin/ziggy-player", "755", None)?;
        }
    }
    Ok(())
}

/// Downloads the wake-word model onto the Pi unless it's already there.
pub fn install_wake_model(r: &dyn Remote) -> anyhow::Result<()> {
    let dir = devconfig::wake_model_dir();
    r.run(&format!(
        "if [ ! -d '{dir}' ]; then\n\
         curl -fsSL '{WAKE_MODEL_URL}/{WAKE_MODEL}.tar.bz2' | sudo tar xj -C '{state}/models'\n\
         sudo chown -R ziggy-player:ziggy-player '{state}/models'\n\
         fi",
        state = devconfig::STATE_DIR
    ))
    .context("downloading the wake-word model")?;
    Ok(())
}

/// The sign-ins and keys the player needs, from this computer's copy.
pub struct Secrets<'a> {
    /// Holds librespot/credentials.json and web-auth.json.
    pub spotify_dir: &'a Path,
    pub vertex_key: Option<&'a Path>,
}

pub fn install_secrets(r: &dyn Remote, secrets: &Secrets) -> anyhow::Result<()> {
    let state = devconfig::STATE_DIR;
    let files = [
        (
            secrets.spotify_dir.join("librespot/credentials.json"),
            format!("{state}/librespot/credentials.json"),
        ),
        (
            secrets.spotify_dir.join(crate::spotify::WEB_AUTH_FILE),
            format!("{state}/{}", crate::spotify::WEB_AUTH_FILE),
        ),
    ];
    for (from, to) in files {
        if from.is_file() {
            r.upload(&std::fs::read(&from)?, &to, "600", Some("ziggy-player"))?;
        }
    }
    if let Some(key) = secrets.vertex_key {
        let bytes = std::fs::read(key).with_context(|| format!("reading {}", key.display()))?;
        r.upload(&bytes, VERTEX_KEY_PATH, "600", Some("ziggy-player"))?;
    }
    Ok(())
}

pub fn write_config(r: &dyn Remote, config: &toml::Table) -> anyhow::Result<()> {
    devconfig::check(config)?;
    r.upload(
        devconfig::render(config).as_bytes(),
        CONFIG_PATH,
        "644",
        None,
    )
}

pub fn read_config(r: &dyn Remote) -> anyhow::Result<Option<toml::Table>> {
    r.read(CONFIG_PATH)?
        .map(|text| devconfig::parse(&text))
        .transpose()
}

/// Restarts the player; returns whether it's running and its last log lines.
pub fn restart(r: &dyn Remote) -> anyhow::Result<(bool, String)> {
    let out = r.run(
        "sudo systemctl restart ziggy-player\n\
         sleep 4\n\
         echo \"STATE=$(systemctl is-active ziggy-player)\"\n\
         sudo journalctl -u ziggy-player -n 12 --no-pager -o cat || true",
    )?;
    Ok((
        out.contains("STATE=active"),
        out.replace("STATE=active\n", ""),
    ))
}

/// True for a player set up before the rename to Ziggy, which still has the muzak names.
pub fn needs_rename_migration(r: &dyn Remote) -> bool {
    r.run("test -d /var/lib/muzak && test ! -d /var/lib/ziggy && echo yes || true")
        .is_ok_and(|out| out.trim() == "yes")
}

/// The installed player's version, e.g. "ziggy-player 0.2.0". A player from before the rename
/// answers as "muzak-player".
pub fn installed_version(r: &dyn Remote) -> Option<String> {
    r.run(
        "/usr/local/bin/ziggy-player --version 2>/dev/null \
         || /usr/local/bin/muzak-player --version 2>/dev/null || true",
    )
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::fake::FakeRemote;

    #[test]
    fn provisioning_uploads_the_scripts_and_reports_a_needed_reboot() {
        let r = FakeRemote {
            output: "Provisioned.\nZIGGY_REBOOT_NEEDED\n".into(),
            ..Default::default()
        };
        assert!(provision(&r).unwrap());
        assert!(
            r.file("/tmp/ziggy-provision/ziggy-player.service")
                .unwrap()
                .contains("ExecStart")
        );
        assert!(r.all_scripts().contains("provision-remote.sh"));
        let again = FakeRemote::default();
        assert!(!provision(&again).unwrap(), "nothing changed, no reboot");
    }

    #[test]
    fn the_player_comes_from_a_release_or_this_computer() {
        let r = FakeRemote::default();
        install_player(&r, &PlayerSource::Release(None)).unwrap();
        assert!(r.all_scripts().contains(
            "https://github.com/mberg/ziggy/releases/latest/download/ziggy-player-linux-arm64.tar.gz"
        ));
        install_player(&r, &PlayerSource::Release(Some("v0.2.0".into()))).unwrap();
        assert!(r.all_scripts().contains("/releases/download/v0.2.0/"));

        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("ziggy-player");
        std::fs::write(&bin, b"\x7fELF").unwrap();
        install_player(&r, &PlayerSource::Local(bin)).unwrap();
        let files = r.files.borrow();
        let (bytes, mode, _) = &files["/usr/local/bin/ziggy-player"];
        assert_eq!((bytes.as_slice(), mode.as_str()), (&b"\x7fELF"[..], "755"));
    }

    #[test]
    fn secrets_go_to_the_ziggy_user_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("librespot")).unwrap();
        std::fs::write(dir.path().join("librespot/credentials.json"), "{}").unwrap();
        std::fs::write(dir.path().join("web-auth.json"), "{}").unwrap();
        let key = dir.path().join("vertex.json");
        std::fs::write(&key, "{\"type\":\"service_account\"}").unwrap();
        let r = FakeRemote::default();
        install_secrets(
            &r,
            &Secrets {
                spotify_dir: dir.path(),
                vertex_key: Some(&key),
            },
        )
        .unwrap();
        for path in [
            "/var/lib/ziggy/librespot/credentials.json",
            "/var/lib/ziggy/web-auth.json",
            "/var/lib/ziggy/vertex-key.json",
        ] {
            let files = r.files.borrow();
            let (_, mode, owner) = &files[path];
            assert_eq!(
                (mode.as_str(), owner.as_deref()),
                ("600", Some("ziggy-player")),
                "{path}"
            );
        }
    }

    #[test]
    fn a_bad_config_is_never_written() {
        let r = FakeRemote::default();
        let mut t = devconfig::new_config(&devconfig::Choices {
            device_name: "Den".into(),
            bitrate: 320,
            audiobookshelf_url: None,
            voice: None,
        });
        write_config(&r, &t).unwrap();
        assert_eq!(
            read_config(&r).unwrap().unwrap()["device_name"].as_str(),
            Some("Den")
        );
        t.insert("initial_volume".into(), toml::Value::Integer(150));
        assert!(write_config(&r, &t).is_err());
    }
}
