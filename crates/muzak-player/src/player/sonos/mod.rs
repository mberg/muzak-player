//! Plays through Sonos speakers on the home network instead of this device.

mod player;
pub mod protocol;

pub use player::{discover_rooms, spawn};
