//! Interface state and key handling, with no terminal or audio dependencies beyond key types.

use std::ops::RangeInclusive;
use std::time::Duration;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::synth::Params;

pub const CARRIER_RANGE: RangeInclusive<f32> = 50.0..=1000.0;
pub const BEAT_RANGE: RangeInclusive<f32> = 0.5..=50.0;
pub const VOLUME_RANGE: RangeInclusive<f32> = 0.0..=100.0;

const CARRIER_STEP_HZ: f32 = 5.0;
const BEAT_STEP_HZ: f32 = 0.5;
const VOLUME_STEP_PCT: i8 = 5;

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

/// The user-adjustable values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub carrier_hz: f32,
    pub beat_hz: f32,
    pub volume_pct: u8,
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

/// A row in the controls panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Control {
    Carrier,
    Beat,
    Volume,
}

impl Control {
    /// Every control, in display order.
    pub const ALL: [Self; 3] = [Self::Carrier, Self::Beat, Self::Volume];
}

/// What the UI shows for one control.
pub struct Readout {
    pub label: &'static str,
    pub fraction: f32,
    pub value: String,
}

/// The whole interface state.
pub struct App {
    pub settings: Settings,
    pub selected: usize,
    pub playing: bool,
    pub should_quit: bool,
    pub play_time: Duration,
    /// Beat cycles elapsed, wrapped to one cycle; drives the envelope display.
    pub beat_phase: f64,
}

impl App {
    /// Creates a playing app from `settings`, with the beat control selected.
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            selected: 1,
            playing: true,
            should_quit: false,
            play_time: Duration::ZERO,
            beat_phase: 0.0,
        }
    }

    /// The audio targets for the current state; pausing is zero gain.
    pub fn params(&self) -> Params {
        let volume = f32::from(self.settings.volume_pct) / 100.0;
        Params {
            carrier_hz: self.settings.carrier_hz,
            beat_hz: self.settings.beat_hz,
            gain: if self.playing { volume } else { 0.0 },
        }
    }

    /// The name of the band containing the current beat frequency.
    pub fn band_name(&self) -> &'static str {
        BANDS
            .iter()
            .find(|band| self.settings.beat_hz < band.below_hz)
            .map_or("gamma", |band| band.name)
    }

    /// Advances the play clock and the envelope display by `elapsed`.
    pub fn tick(&mut self, elapsed: Duration) {
        if self.playing {
            self.play_time += elapsed;
            let cycles = f64::from(self.settings.beat_hz) * elapsed.as_secs_f64();
            self.beat_phase = (self.beat_phase + cycles).fract();
        }
    }

    /// Applies one key press.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if is_quit(key) {
            self.should_quit = true;
            return;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(Control::ALL.len() - 1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Left | KeyCode::Char('h') => self.adjust(-1),
            KeyCode::Right | KeyCode::Char('l') => self.adjust(1),
            KeyCode::Char(' ') => self.playing = !self.playing,
            KeyCode::Char(digit @ '1'..='5') => {
                self.settings.beat_hz = BANDS[digit as usize - '1' as usize].preset_hz;
            }
            _ => {}
        }
    }

    /// Label, slider position and formatted value for `control`.
    pub fn readout(&self, control: Control) -> Readout {
        let Settings {
            carrier_hz,
            beat_hz,
            volume_pct,
        } = self.settings;
        match control {
            Control::Carrier => Readout {
                label: "carrier",
                fraction: fraction_of(carrier_hz, &CARRIER_RANGE),
                value: format!("{carrier_hz:.1} Hz"),
            },
            Control::Beat => Readout {
                label: "beat",
                fraction: fraction_of(beat_hz, &BEAT_RANGE),
                value: format!("{beat_hz:.1} Hz"),
            },
            Control::Volume => Readout {
                label: "volume",
                fraction: fraction_of(f32::from(volume_pct), &VOLUME_RANGE),
                value: format!("{volume_pct}%"),
            },
        }
    }

    /// Moves the selection forward by `offset` rows, wrapping around.
    fn move_selection(&mut self, offset: usize) {
        self.selected = (self.selected + offset) % Control::ALL.len();
    }

    /// Nudges the selected control one step in `direction` (-1 or 1), staying in range.
    fn adjust(&mut self, direction: i8) {
        let sign = f32::from(direction);
        let settings = &mut self.settings;
        match Control::ALL[self.selected] {
            Control::Carrier => {
                let next = settings.carrier_hz + sign * CARRIER_STEP_HZ;
                settings.carrier_hz = next.clamp(*CARRIER_RANGE.start(), *CARRIER_RANGE.end());
            }
            Control::Beat => {
                let next = settings.beat_hz + sign * BEAT_STEP_HZ;
                settings.beat_hz = next.clamp(*BEAT_RANGE.start(), *BEAT_RANGE.end());
            }
            Control::Volume => {
                let step = direction * VOLUME_STEP_PCT;
                settings.volume_pct = settings.volume_pct.saturating_add_signed(step).min(100);
            }
        }
    }
}

/// Whether `key` exits the app; raw mode delivers Ctrl-C as a key instead of a signal.
fn is_quit(key: KeyEvent) -> bool {
    let ctrl_c = key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
    ctrl_c || matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
}

/// Position of `value` within `range`, from 0 to 1.
fn fraction_of(value: f32, range: &RangeInclusive<f32>) -> f32 {
    (value - range.start()) / (range.end() - range.start())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::app::{App, Control, Settings};

    /// Sends a plain key press to `app`.
    fn press(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    /// Selects `control` directly.
    fn select(app: &mut App, control: Control) {
        app.selected = Control::ALL.iter().position(|c| *c == control).unwrap();
    }

    #[test]
    fn presets_set_the_beat_to_their_own_band() {
        let mut app = App::new(Settings::default());
        for (key, name, hz) in [
            ('1', "delta", 2.0),
            ('2', "theta", 6.0),
            ('3', "alpha", 10.0),
            ('4', "beta", 20.0),
            ('5', "gamma", 40.0),
        ] {
            press(&mut app, KeyCode::Char(key));
            assert_eq!(app.settings.beat_hz, hz);
            assert_eq!(app.band_name(), name);
        }
    }

    #[test]
    fn controls_stop_at_their_limits() {
        let mut app = App::new(Settings::default());
        select(&mut app, Control::Volume);
        for _ in 0..30 {
            press(&mut app, KeyCode::Right);
        }
        assert_eq!(app.settings.volume_pct, 100);
        for _ in 0..30 {
            press(&mut app, KeyCode::Left);
        }
        assert_eq!(app.settings.volume_pct, 0);

        select(&mut app, Control::Beat);
        for _ in 0..200 {
            press(&mut app, KeyCode::Left);
        }
        assert_eq!(app.settings.beat_hz, 0.5);

        select(&mut app, Control::Carrier);
        for _ in 0..300 {
            press(&mut app, KeyCode::Right);
        }
        assert_eq!(app.settings.carrier_hz, 1000.0);
    }

    #[test]
    fn selection_wraps_both_ways() {
        let mut app = App::new(Settings::default());
        assert_eq!(Control::ALL[app.selected], Control::Beat);
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Up);
        assert_eq!(Control::ALL[app.selected], Control::Volume);
        press(&mut app, KeyCode::Down);
        assert_eq!(Control::ALL[app.selected], Control::Carrier);
    }

    #[test]
    fn pausing_mutes_and_stops_the_clock() {
        let mut app = App::new(Settings::default());
        app.tick(Duration::from_secs(2));
        assert_eq!(app.params().gain, 0.25);
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(app.params().gain, 0.0);
        app.tick(Duration::from_secs(5));
        assert_eq!(app.play_time, Duration::from_secs(2));
    }

    #[test]
    fn q_escape_and_ctrl_c_quit_but_plain_c_does_not() {
        for key in [
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            let mut app = App::new(Settings::default());
            app.handle_key(key);
            assert!(app.should_quit, "{key:?}");
        }
        let mut app = App::new(Settings::default());
        press(&mut app, KeyCode::Char('c'));
        assert!(!app.should_quit);
    }
}
