//! Playback state for the menu bar app, kept free of AppKit so it can be tested directly.
//!
//! The stream only exists while playing: pausing fades out and closes it, so a paused app does
//! no audio work and lets the output device go idle.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use binaural::audio::{self, AudioOutput, StreamEvent};
use binaural::logging::{Level, Logger};
use binaural::settings::{Settings, band_name};
use binaural::synth::SharedParams;

pub const BEAT_CHOICES: [f32; 6] = [2.0, 6.0, 10.0, 15.0, 20.0, 40.0];
pub const CARRIER_CHOICES: [f32; 6] = [100.0, 150.0, 200.0, 250.0, 300.0, 400.0];
pub const VOLUME_CHOICES: [u8; 8] = [5, 10, 15, 20, 25, 30, 40, 50];

/// Time allowed for audio to fade out before the stream closes, so stopping does not click.
const FADE_OUT: Duration = Duration::from_millis(200);

/// Settings, the open stream if playing, and the last error to show.
pub struct Player {
    settings: Settings,
    shared: Arc<SharedParams>,
    output: Option<AudioOutput>,
    /// Name of the output in use; tracks reroutes, unlike the name the stream opened with.
    device: Option<String>,
    /// Increments per opened stream, so events from a closed stream can be told apart.
    stream_id: u64,
    logger: Logger,
    error: Option<String>,
    /// Why playback stopped on its own when that was not a failure, such as an output switch.
    notice: Option<String>,
    play_started: Option<Instant>,
    played: Duration,
}

impl Player {
    /// Creates a paused player.
    pub fn new(settings: Settings, logger: Logger) -> Self {
        Self {
            settings,
            shared: Arc::new(SharedParams::new(settings.params(false))),
            output: None,
            device: None,
            stream_id: 0,
            logger,
            error: None,
            notice: None,
            play_started: None,
            played: Duration::ZERO,
        }
    }

    /// The current settings.
    pub fn settings(&self) -> Settings {
        self.settings
    }

    /// Whether a stream is open.
    pub fn is_playing(&self) -> bool {
        self.output.is_some()
    }

    /// Name of the output in use while playing.
    pub fn device(&self) -> Option<&str> {
        self.device.as_deref()
    }

    /// The most recent failure, cleared by the next successful Play.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// What the menu should explain: the failure if there is one, else why playback paused.
    pub fn message(&self) -> Option<&str> {
        self.error.as_deref().or(self.notice.as_deref())
    }

    /// Appends to this run's log.
    pub fn log(&mut self, level: Level, message: &str) -> Result<(), String> {
        self.logger.log(level, message)
    }

    /// Logs `error` and keeps it for the menu, noting a logging failure there too.
    pub fn record_error(&mut self, error: String) {
        self.error = Some(match self.logger.log(Level::Error, &error) {
            Ok(()) => error,
            Err(log_error) => format!("{error}; also {log_error}"),
        });
    }

    /// Opens the current default output and fades in.
    ///
    /// `on_event` runs on the audio thread with this stream's id; pass the event back through
    /// [`Player::stream_event`] on the main thread.
    pub fn play(
        &mut self,
        on_event: impl Fn(u64, StreamEvent) + Send + 'static,
    ) -> Result<(), String> {
        if self.is_playing() {
            return Ok(());
        }
        self.stream_id += 1;
        let stream_id = self.stream_id;
        self.shared.store(self.settings.params(true));
        let output = audio::start(Arc::clone(&self.shared), move |event| {
            on_event(stream_id, event);
        })?;
        let message = format!(
            "audio started: device `{}`, {} Hz, {} channels, {:?}",
            output.device, output.sample_rate, output.channels, self.settings
        );
        self.device = Some(output.device.clone());
        self.output = Some(output);
        self.error = None;
        self.notice = None;
        self.play_started = Some(Instant::now());
        self.log(Level::Info, &message)
    }

    /// Fades out, then closes the stream.
    pub fn pause(&mut self) -> Result<(), String> {
        if !self.is_playing() {
            return Ok(());
        }
        self.shared.store(self.settings.params(false));
        thread::sleep(FADE_OUT);
        self.close_stream();
        self.log(Level::Info, "audio stopped")
    }

    /// Applies `change` to the settings; a playing stream glides to the new values.
    pub fn update(&mut self, change: impl FnOnce(&mut Settings)) -> Result<(), String> {
        change(&mut self.settings);
        self.shared.store(self.settings.params(self.is_playing()));
        let message = format!("changed: {:?}", self.settings);
        self.log(Level::Debug, &message)
    }

    /// Handles an event from stream `stream_id`. Events from a stream already closed are ignored.
    pub fn stream_event(&mut self, stream_id: u64, event: StreamEvent) -> Result<(), String> {
        if stream_id != self.stream_id || !self.is_playing() {
            return Ok(());
        }
        let device = self.device.clone().unwrap_or_else(|| "unknown".into());
        if event.fatal {
            self.close_stream();
            return Err(format!(
                "audio stream on `{device}` failed: {}; choose Play to reopen the current output",
                event.message
            ));
        }
        if event.rerouted {
            // Stop at once rather than fading: the new output is often the laptop speakers.
            self.close_stream();
            let new_device = audio::default_output_name().unwrap_or_else(|| "unknown".into());
            let notice = format!("Paused: output switched to {new_device}");
            let message = format!("{notice} from `{device}`");
            self.notice = Some(notice);
            return self.log(Level::Info, &message);
        }
        let message = format!("stream event on `{device}`: {}", event.message);
        self.log(Level::Warn, &message)
    }

    /// Stops playback and logs the session total.
    pub fn quit(&mut self) -> Result<(), String> {
        self.pause()?;
        let message = format!(
            "session complete: played {:.1} s",
            self.played.as_secs_f32()
        );
        self.log(Level::Info, &message)
    }

    /// Drops the stream and adds its run time to the session total.
    fn close_stream(&mut self) {
        self.output = None;
        self.device = None;
        if let Some(started) = self.play_started.take() {
            self.played += started.elapsed();
        }
    }
}

/// A frequency without a trailing `.0`, such as "15 Hz" or "142.5 Hz".
pub fn hz_label(hz: f32) -> String {
    if hz.fract() == 0.0 {
        format!("{hz:.0} Hz")
    } else {
        format!("{hz:.1} Hz")
    }
}

/// A beat frequency with its band, such as "15 Hz · beta".
pub fn beat_label(hz: f32) -> String {
    format!("{} · {}", hz_label(hz), band_name(hz))
}

/// Per-ear frequencies, such as "L 142.5 Hz · R 157.5 Hz".
pub fn ears_label(settings: Settings) -> String {
    let params = settings.params(false);
    format!(
        "L {} · R {}",
        hz_label(params.left_hz()),
        hz_label(params.right_hz())
    )
}

/// Text in the menu bar: the beat while playing, a mark after a failure, the glyph alone when idle.
pub fn status_title(settings: Settings, playing: bool, failed: bool) -> String {
    match (playing, failed) {
        (true, _) => format!("∿ {}", hz_label(settings.beat_hz)),
        (false, true) => "∿ !".into(),
        (false, false) => "∿".into(),
    }
}

#[cfg(test)]
mod tests {
    use binaural::audio::StreamEvent;
    use binaural::logging::Logger;
    use binaural::settings::Settings;

    use crate::player::{
        BEAT_CHOICES, CARRIER_CHOICES, Player, VOLUME_CHOICES, beat_label, ears_label, hz_label,
        status_title,
    };

    /// A paused player logging to a temporary folder.
    fn player() -> Player {
        let logger = Logger::create(&std::env::temp_dir().join("binaural-bar-tests")).unwrap();
        Player::new(Settings::default(), logger)
    }

    #[test]
    fn defaults_are_menu_choices() {
        let settings = Settings::default();
        assert!(BEAT_CHOICES.contains(&settings.beat_hz));
        assert!(CARRIER_CHOICES.contains(&settings.carrier_hz));
        assert!(VOLUME_CHOICES.contains(&settings.volume_pct));
    }

    #[test]
    fn labels_drop_trailing_zeroes() {
        assert_eq!(hz_label(15.0), "15 Hz");
        assert_eq!(hz_label(142.5), "142.5 Hz");
        assert_eq!(beat_label(15.0), "15 Hz · beta");
        let settings = Settings {
            carrier_hz: 150.0,
            beat_hz: 15.0,
            volume_pct: 20,
        };
        assert_eq!(ears_label(settings), "L 142.5 Hz · R 157.5 Hz");
    }

    #[test]
    fn status_title_shows_beat_only_while_playing() {
        let settings = Settings::default();
        assert_eq!(status_title(settings, true, false), "∿ 10 Hz");
        assert_eq!(status_title(settings, false, false), "∿");
        assert_eq!(status_title(settings, false, true), "∿ !");
    }

    #[test]
    fn events_from_a_closed_stream_are_ignored() {
        let mut player = player();
        let event = StreamEvent {
            message: "gone".into(),
            fatal: true,
            rerouted: false,
        };
        assert_eq!(player.stream_event(1, event), Ok(()));
        assert!(player.error().is_none());
    }

    #[test]
    fn updates_apply_while_paused_and_pause_is_idempotent() {
        let mut player = player();
        player.update(|settings| settings.beat_hz = 15.0).unwrap();
        assert_eq!(player.settings().beat_hz, 15.0);
        assert_eq!(player.pause(), Ok(()));
        assert!(!player.is_playing());
    }

    #[test]
    fn recorded_errors_are_kept_for_the_menu() {
        let mut player = player();
        player.record_error("no output".into());
        assert_eq!(player.error(), Some("no output"));
        assert_eq!(player.message(), Some("no output"));
    }
}
