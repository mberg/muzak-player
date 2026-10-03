/// librespot volume is 0..=65535; the app uses percent.
pub fn percent_to_volume(percent: u8) -> u16 {
    ((percent.min(100) as u32 * u16::MAX as u32 + 50) / 100) as u16
}

pub fn volume_to_percent(volume: u16) -> u8 {
    ((volume as u32 * 100 + u16::MAX as u32 / 2) / u16::MAX as u32) as u8
}

pub mod fake;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_conversions_roundtrip() {
        assert_eq!(percent_to_volume(0), 0);
        assert_eq!(percent_to_volume(100), u16::MAX);
        assert_eq!(percent_to_volume(250), u16::MAX);
        for p in 0..=100u8 {
            assert_eq!(volume_to_percent(percent_to_volume(p)), p);
        }
    }
}
