//! In-memory catalog for `--fake` mode: UI work without a Spotify account.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::{FetchError, LibrarySource};
use crate::model::{
    Account, Collection, CollectionKind, LIKED_URI, Section, Track, liked_collection,
};

pub struct FakeCatalog {
    pub playlists: Vec<Collection>,
    pub albums: Vec<Collection>,
    tracks: HashMap<String, Vec<Track>>,
}

impl FakeCatalog {
    pub fn sample() -> Self {
        let playlist_names = [
            "Dance Party",
            "Bedtime",
            "Car Songs",
            "Frozen Favourites",
            "Morning Mix",
            "A Very Long Playlist Name That Should Be Cut Off Nicely",
        ];
        let album_names = [
            ("Moana", "Various Artists"),
            ("Encanto", "Various Artists"),
            ("Abbey Road", "The Beatles"),
            ("Rumours", "Fleetwood Mac"),
        ];
        let playlists: Vec<Collection> = playlist_names
            .iter()
            .enumerate()
            .map(|(i, name)| Collection {
                uri: format!("spotify:playlist:fake{i}"),
                kind: CollectionKind::Playlist,
                name: name.to_string(),
                subtitle: "Mum".into(),
                image_url: None,
            })
            .collect();
        let albums: Vec<Collection> = album_names
            .iter()
            .enumerate()
            .map(|(i, (name, artist))| Collection {
                uri: format!("spotify:album:fake{i}"),
                kind: CollectionKind::Album,
                name: name.to_string(),
                subtitle: artist.to_string(),
                image_url: None,
            })
            .collect();
        let mut tracks = HashMap::new();
        for c in playlists.iter().chain(albums.iter()) {
            tracks.insert(c.uri.clone(), fake_tracks(&c.uri, &c.name, 12));
        }
        tracks.insert(LIKED_URI.to_string(), fake_tracks(LIKED_URI, "Liked", 20));
        Self {
            playlists,
            albums,
            tracks,
        }
    }

    pub fn tracks_for(&self, uri: &str) -> Vec<Track> {
        self.tracks.get(uri).cloned().unwrap_or_default()
    }
}

fn fake_tracks(collection_uri: &str, collection: &str, n: u32) -> Vec<Track> {
    (1..=n)
        .map(|i| Track {
            uri: format!(
                "spotify:track:fake-{}-{i}",
                collection_uri.replace(':', "-")
            ),
            name: format!("{collection} Song {i}"),
            artists: "The Fake Band".into(),
            album: collection.to_string(),
            image_url: None,
            // Short tracks so auto-advance is visible during UI work.
            duration_ms: 20_000 + i * 1_000,
        })
        .collect()
}

pub struct FakeSource {
    catalog: Arc<FakeCatalog>,
}

impl FakeSource {
    pub fn new(catalog: Arc<FakeCatalog>) -> Self {
        Self { catalog }
    }
}

impl LibrarySource for FakeSource {
    async fn section(&self, section: Section) -> Result<Vec<Collection>, FetchError> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        Ok(match section {
            Section::Playlists => self.catalog.playlists.clone(),
            Section::Albums => self.catalog.albums.clone(),
            Section::Recent => {
                let mut recent = self.catalog.playlists[..2].to_vec();
                recent.push(self.catalog.albums[0].clone());
                recent.push(liked_collection());
                recent
            }
            Section::Liked => vec![liked_collection()],
        })
    }

    async fn tracks(&self, collection_uri: &str) -> Result<Vec<Track>, FetchError> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        Ok(self.catalog.tracks_for(collection_uri))
    }

    async fn account(&self) -> Result<Account, FetchError> {
        Ok(Account {
            id: "fake".into(),
            name: "Fake Account".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn track_uris_are_unique_across_the_catalog() {
        let catalog = FakeCatalog::sample();
        let mut seen = HashSet::new();
        let uris = catalog
            .playlists
            .iter()
            .chain(catalog.albums.iter())
            .map(|c| c.uri.as_str())
            .chain([LIKED_URI]);
        for uri in uris {
            for track in catalog.tracks_for(uri) {
                assert!(seen.insert(track.uri.clone()), "duplicate {}", track.uri);
            }
        }
        assert!(seen.len() > 100);
    }
}
