//! Tiny JSON-file cache so screens show instantly and work offline.

use std::path::PathBuf;

use serde::Serialize;
use serde::de::DeserializeOwned;

pub struct DiskCache {
    dir: PathBuf,
}

pub fn tracks_key(uri: &str) -> String {
    format!("tracks-{uri}")
}

impl DiskCache {
    pub fn new(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    pub fn path_for(&self, key: &str) -> PathBuf {
        let safe: String = key
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.join(format!("{safe}.json"))
    }

    pub fn read<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let bytes = std::fs::read(self.path_for(key)).ok()?;
        match serde_json::from_slice(&bytes) {
            Ok(value) => Some(value),
            Err(e) => {
                tracing::warn!(key, "ignoring corrupt cache entry: {e}");
                None
            }
        }
    }

    /// How long ago an entry was written, if it exists.
    pub fn age(&self, key: &str) -> Option<std::time::Duration> {
        let modified = std::fs::metadata(self.path_for(key))
            .ok()?
            .modified()
            .ok()?;
        modified.elapsed().ok()
    }

    pub fn write<T: Serialize>(&self, key: &str, value: &T) -> std::io::Result<()> {
        let path = self.path_for(key);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(value)?)?;
        std::fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_missing_and_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path().join("c")).unwrap();
        assert_eq!(cache.read::<Vec<u32>>("k"), None);
        cache.write("k", &vec![1u32, 2]).unwrap();
        assert_eq!(cache.read::<Vec<u32>>("k"), Some(vec![1, 2]));
        std::fs::write(cache.path_for("k"), b"{not json").unwrap();
        assert_eq!(cache.read::<Vec<u32>>("k"), None);
    }

    #[test]
    fn keys_are_sanitized() {
        let cache = DiskCache::new(tempfile::tempdir().unwrap().path()).unwrap();
        let path = cache.path_for(&tracks_key("spotify:playlist:a/b"));
        assert_eq!(
            path.file_name().unwrap(),
            "tracks-spotify_playlist_a_b.json"
        );
    }
}
