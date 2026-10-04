//! Glue between the runtime (any thread) and the Slint window (UI thread).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use image::RgbaImage;
use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use tokio::sync::mpsc::UnboundedSender;

use crate::app::{AppState, DisplayMode, Input, UiAction};
use crate::model::Section;
use crate::view::{self, LoadStatus, ScreenView, TileView, TrackRowView};
use crate::{AppWindow, LoadState, ScreenKind, TileData, TrackData};

const IMAGE_CACHE_LIMIT: usize = 80;

struct Images {
    ready: HashMap<String, Image>,
    order: VecDeque<String>,
    requested: HashSet<String>,
    /// URLs the current view shows. These are never evicted.
    live: HashSet<String>,
    requests: UnboundedSender<String>,
}

/// Removes and returns the oldest URLs until `order` fits in `limit`, skipping live ones.
/// When everything left is live the cache stays over the limit: evicting a cover on screen
/// would only make the next render request it again.
fn evict(order: &mut VecDeque<String>, live: &HashSet<String>, limit: usize) -> Vec<String> {
    let mut evicted = Vec::new();
    let mut i = 0;
    while order.len() > limit && i < order.len() {
        if live.contains(&order[i]) {
            i += 1;
        } else {
            evicted.extend(order.remove(i));
        }
    }
    evicted
}

impl Images {
    fn get(&mut self, url: &Option<String>) -> Option<Image> {
        let url = url.as_ref()?;
        if let Some(image) = self.ready.get(url) {
            return Some(image.clone());
        }
        if self.requested.insert(url.clone()) {
            let _ = self.requests.send(url.clone());
        }
        None
    }

    fn insert(&mut self, url: String, image: Image) {
        self.ready.insert(url.clone(), image);
        self.order.push_back(url);
        for old in evict(&mut self.order, &self.live, IMAGE_CACHE_LIMIT) {
            self.ready.remove(&old);
            self.requested.remove(&old);
        }
    }
}

struct Bridge {
    window: slint::Weak<AppWindow>,
    images: Images,
    tiles: Rc<VecModel<TileData>>,
    tracks: Rc<VecModel<TrackData>>,
    last: Option<AppState>,
}

thread_local! {
    static BRIDGE: RefCell<Option<Bridge>> = const { RefCell::new(None) };
}

fn with_bridge<R>(f: impl FnOnce(&mut Bridge) -> R) -> Option<R> {
    BRIDGE.with(|cell| cell.borrow_mut().as_mut().map(f))
}

/// Call once on the UI thread before `window.run()`.
pub fn install(window: &AppWindow, image_requests: UnboundedSender<String>) {
    let tiles = Rc::new(VecModel::<TileData>::default());
    let tracks = Rc::new(VecModel::<TrackData>::default());
    window.set_tiles(ModelRc::from(tiles.clone()));
    window.set_detail_tracks(ModelRc::from(tracks.clone()));
    let bridge = Bridge {
        window: window.as_weak(),
        images: Images {
            ready: HashMap::new(),
            order: VecDeque::new(),
            requested: HashSet::new(),
            live: HashSet::new(),
            requests: image_requests,
        },
        tiles,
        tracks,
        last: None,
    };
    BRIDGE.with(|cell| *cell.borrow_mut() = Some(bridge));
}

/// Called from the runtime thread with each new state.
pub fn publish(state: AppState) {
    let _ = slint::invoke_from_event_loop(move || {
        with_bridge(|b| {
            b.last = Some(state);
            b.render();
        });
    });
}

/// Called from the image loader with a decoded cover.
pub fn deliver_image(url: String, image: Option<RgbaImage>) {
    let Some(image) = image else { return };
    let (width, height) = image.dimensions();
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(image.as_raw(), width, height);
    let _ = slint::invoke_from_event_loop(move || {
        with_bridge(|b| {
            b.images.insert(url, Image::from_rgba8(buffer));
            b.render();
        });
    });
}

pub fn wire_callbacks(window: &AppWindow, inputs: UnboundedSender<Input>) {
    let send = move |action: UiAction| {
        let _ = inputs.send(Input::Ui(action));
    };
    let s = send.clone();
    window.on_show_section(move |i| s(UiAction::ShowSection(Section::from_index(i))));
    let s = send.clone();
    window.on_open_collection(move |uri| s(UiAction::OpenCollection(uri.to_string())));
    let s = send.clone();
    window.on_back(move || s(UiAction::Back));
    let s = send.clone();
    window.on_open_now_playing(move || s(UiAction::OpenNowPlaying));
    let s = send.clone();
    window.on_play_collection(move |uri, shuffle| {
        s(UiAction::PlayCollection {
            uri: uri.to_string(),
            shuffle,
        })
    });
    let s = send.clone();
    window.on_play_track(move |uri, index| {
        s(UiAction::PlayTrack {
            collection_uri: uri.to_string(),
            index: index.max(0) as usize,
        })
    });
    let s = send.clone();
    window.on_toggle_play(move || s(UiAction::TogglePlay));
    let s = send.clone();
    window.on_next(move || s(UiAction::Next));
    let s = send.clone();
    window.on_previous(move || s(UiAction::Previous));
    let s = send.clone();
    window.on_seek(move |fraction| {
        let duration = with_bridge(|b| {
            b.last
                .as_ref()
                .and_then(|st| st.playback.track.as_ref())
                .map(|t| t.duration_ms)
        })
        .flatten()
        .unwrap_or(0);
        s(UiAction::Seek {
            position_ms: (fraction.clamp(0.0, 1.0) * duration as f32) as u32,
        });
    });
    let s = send.clone();
    window.on_set_volume(move |percent| {
        s(UiAction::SetVolume {
            percent: percent.clamp(0, 100) as u8,
        })
    });
    let s = send.clone();
    window.on_toggle_shuffle(move || s(UiAction::ToggleShuffle));
    let s = send.clone();
    window.on_cycle_repeat(move || s(UiAction::CycleRepeat));
    let s = send;
    window.on_touched(move || s(UiAction::Touch));
}

impl Bridge {
    fn render(&mut self) {
        let Some(state) = self.last.as_ref() else {
            return;
        };
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let v = view::build(state);
        self.images.live = live_images(&v);

        w.set_screen(match v.screen {
            ScreenView::Grid => ScreenKind::Grid,
            ScreenView::Detail => ScreenKind::Detail,
            ScreenView::NowPlaying => ScreenKind::NowPlaying,
        });
        w.set_section(v.section.index());
        w.set_grid_title(v.grid_title.as_str().into());
        let tiles: Vec<TileData> = v
            .grid
            .iter()
            .map(|t| tile_data(&mut self.images, t))
            .collect();
        sync(&self.tiles, tiles);
        w.set_grid_state(load_state(v.grid_status));

        if let Some(detail) = &v.detail {
            w.set_detail(tile_data(&mut self.images, &detail.header));
            sync(&self.tracks, detail.tracks.iter().map(track_data).collect());
            w.set_detail_state(load_state(detail.status));
        }

        let art = self.images.get(&v.now.image_url);
        w.set_mini_visible(v.mini_visible);
        w.set_track_title(v.now.title.as_str().into());
        w.set_track_artists(v.now.artists.as_str().into());
        w.set_track_has_art(art.is_some());
        w.set_track_art(art.unwrap_or_default());
        w.set_playing(v.now.playing);
        w.set_loading(v.now.loading);
        w.set_progress(v.now.progress);
        w.set_position_text(v.now.position.as_str().into());
        w.set_duration_text(v.now.duration.as_str().into());
        w.set_volume(v.now.volume as i32);
        w.set_shuffle(v.now.shuffle);
        w.set_repeat(v.now.repeat.index());
        w.set_banner(v.banner.unwrap_or_default().into());
        w.set_display_mode(match v.display {
            DisplayMode::Active => 0,
            DisplayMode::Dim => 1,
            DisplayMode::Off => 2,
        });
        w.set_clock(chrono::Local::now().format("%-I:%M").to_string().into());
        w.set_auth_needed(v.auth_needed);
    }
}

/// Every image URL the view shows: grid tiles, the detail header and the now-playing art.
fn live_images(v: &view::View) -> HashSet<String> {
    v.grid
        .iter()
        .map(|t| &t.image_url)
        .chain(v.detail.iter().map(|d| &d.header.image_url))
        .chain(std::iter::once(&v.now.image_url))
        .flatten()
        .cloned()
        .collect()
}

fn tile_data(images: &mut Images, t: &TileView) -> TileData {
    let image = images.get(&t.image_url);
    TileData {
        uri: t.uri.as_str().into(),
        title: t.title.as_str().into(),
        subtitle: t.subtitle.as_str().into(),
        has_image: image.is_some(),
        image: image.unwrap_or_default(),
    }
}

fn track_data(t: &TrackRowView) -> TrackData {
    TrackData {
        title: t.title.as_str().into(),
        artists: t.artists.as_str().into(),
        duration: t.duration.as_str().into(),
        current: t.current,
    }
}

fn load_state(status: LoadStatus) -> LoadState {
    match status {
        LoadStatus::Loading => LoadState::Loading,
        LoadStatus::Empty => LoadState::Empty,
        LoadStatus::Failed => LoadState::Failed,
        LoadStatus::Ready => LoadState::Ready,
    }
}

/// Updates only changed rows so scroll positions and images don't reset.
fn sync<T: Clone + PartialEq + 'static>(model: &VecModel<T>, rows: Vec<T>) {
    if model.row_count() != rows.len() {
        model.set_vec(rows);
        return;
    }
    for (i, row) in rows.into_iter().enumerate() {
        if model.row_data(i).as_ref() != Some(&row) {
            model.set_row_data(i, row);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(range: std::ops::Range<usize>) -> Vec<String> {
        range
            .map(|i| format!("https://i.scdn.co/image/{i}"))
            .collect()
    }

    #[test]
    fn eviction_drops_oldest_but_skips_live() {
        let mut order: VecDeque<String> = urls(0..5).into();
        let live: HashSet<String> = urls(0..2).into_iter().collect();
        let evicted = evict(&mut order, &live, 3);
        assert_eq!(evicted, urls(2..4));
        assert_eq!(Vec::from(order), [urls(0..2), urls(4..5)].concat());
    }

    #[test]
    fn eviction_keeps_everything_when_all_live() {
        let mut order: VecDeque<String> = urls(0..5).into();
        let live: HashSet<String> = urls(0..5).into_iter().collect();
        assert!(evict(&mut order, &live, 3).is_empty());
        assert_eq!(order.len(), 5);
    }

    // A grid with more covers than the cache holds must not reload covers in a loop.
    #[test]
    fn live_covers_over_the_limit_are_not_requested_again() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut images = Images {
            ready: HashMap::new(),
            order: VecDeque::new(),
            requested: HashSet::new(),
            live: HashSet::new(),
            requests: tx,
        };
        let all = urls(0..IMAGE_CACHE_LIMIT + 1);
        images.live = all.iter().cloned().collect();
        for url in &all {
            assert!(images.get(&Some(url.clone())).is_none());
        }
        for url in &all {
            images.insert(url.clone(), Image::default());
            // Each delivery re-renders the view.
            for u in &all {
                images.get(&Some(u.clone()));
            }
        }
        let mut sent = 0;
        while rx.try_recv().is_ok() {
            sent += 1;
        }
        assert_eq!(sent, all.len(), "each cover requested once");
        assert!(all.iter().all(|u| images.ready.contains_key(u)));
    }
}
