//! User-facing settings, their limits, and the EEG band names shared by both front ends.

use std::ops::RangeInclusive;

use crate::synth::Params;

pub const CARRIER_RANGE: RangeInclusive<f32> = 50.0..=1000.0;
pub const BEAT_RANGE: RangeInclusive<f32> = 0.5..=50.0;
pub const VOLUME_RANGE: RangeInclusive<f32> = 0.0..=100.0;

/// EEG band names with the beat frequency each preset jumps to and the band's upper limit.
pub const BANDS: [Band; 5] = [
    Band {
        name: "delta",
        preset_hz: 2.0,
        below_hz: 4.0,
    },
    Band {
        name: "theta",
        preset_hz: 6.0,
        below_hz: 8.0,
    },
    Band {
        name: "alpha",
        preset_hz: 10.0,
        below_hz: 13.0,
    },
    Band {
        name: "beta",
        preset_hz: 20.0,
        below_hz: 30.0,
    },
    Band {
        name: "gamma",
        preset_hz: 40.0,
        below_hz: f32::INFINITY,
    },
];

/// A named beat-frequency band.
pub struct Band {
    pub name: &'static str,
    pub preset_hz: f32,
    below_hz: f32,
}

/// The name of the band containing `beat_hz`.
pub fn band_name(beat_hz: f32) -> &'static str {
    BANDS
        .iter()
        .find(|band| beat_hz < band.below_hz)
        .map_or("gamma", |band| band.name)
}

/// The user-adjustable values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub carrier_hz: f32,
    pub beat_hz: f32,
    pub volume_pct: u8,
}

impl Settings {
    /// Audio targets for these settings, silent unless `playing`.
    pub fn params(self, playing: bool) -> Params {
        let volume = f32::from(self.volume_pct) / 100.0;
        Params {
            carrier_hz: self.carrier_hz,
            beat_hz: self.beat_hz,
            gain: if playing { volume } else { 0.0 },
        }
    }
}

impl Default for Settings {
    /// A low carrier, an alpha beat, and a quiet starting volume.
    fn default() -> Self {
        Self {
            carrier_hz: 150.0,
            beat_hz: 10.0,
            volume_pct: 25,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::{Settings, band_name};

    #[test]
    fn band_edges_belong_to_the_higher_band() {
        assert_eq!(band_name(3.9), "delta");
        assert_eq!(band_name(4.0), "theta");
        assert_eq!(band_name(13.0), "beta");
        assert_eq!(band_name(15.0), "beta");
        assert_eq!(band_name(30.0), "gamma");
    }

    #[test]
    fn params_are_silent_when_not_playing() {
        let settings = Settings::default();
        assert_eq!(settings.params(true).gain, 0.25);
        assert_eq!(settings.params(false).gain, 0.0);
    }
}
