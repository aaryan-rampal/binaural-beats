//! Binaural beat generator with a terminal interface.

mod app;
mod args;
mod audio;
mod logging;
mod synth;
mod ui;

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use crate::app::{App, Settings};
use crate::args::Command;
use crate::logging::{Level, Logger};
use crate::synth::{Params, SharedParams};

/// Redraw interval, about 30 frames per second.
const FRAME: Duration = Duration::from_millis(33);
/// Time allowed for audio to fade out on exit, so stopping does not click.
const FADE_OUT: Duration = Duration::from_millis(200);

fn main() -> ExitCode {
    let settings = match args::parse(std::env::args().skip(1)) {
        Ok(Command::Run(settings)) => settings,
        Ok(Command::Help) => {
            println!("{}", args::USAGE);
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    match run(settings) {
        Ok(summary) => {
            eprintln!("{summary}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs one logged session and returns the line to print after the interface closes.
fn run(settings: Settings) -> Result<String, String> {
    let mut logger = Logger::create(Path::new("logs"))?;
    let log_path = logger.path().display().to_string();
    logger.log(Level::Info, &format!("session start: {settings:?}"))?;

    let started = Instant::now();
    match play(settings, &mut logger) {
        Ok(play_time) => {
            let session = started.elapsed().as_secs_f32();
            let played = play_time.as_secs_f32();
            let summary = format!("played {played:.1} s of a {session:.1} s session");
            logger.log(Level::Info, &format!("session complete: {summary}"))?;
            Ok(format!("{summary}; log: {log_path}"))
        }
        Err(error) => {
            logger.log(Level::Error, &format!("session failed: {error}"))?;
            Err(format!("{error} (log: {log_path})"))
        }
    }
}

/// Starts audio, runs the interface until quit, and returns the time spent playing.
fn play(settings: Settings, logger: &mut Logger) -> Result<Duration, String> {
    let mut app = App::new(settings);
    let shared = Arc::new(SharedParams::new(app.params()));
    let (error_sender, stream_errors) = mpsc::channel();
    let output = audio::start(Arc::clone(&shared), error_sender)?;
    logger.log(
        Level::Info,
        &format!(
            "audio started: device `{}`, {} Hz, {} channels",
            output.device, output.sample_rate, output.channels
        ),
    )?;

    let mut terminal = ratatui::try_init().map_err(|error| {
        format!("starting terminal interface: {error}; run this in an interactive terminal")
    })?;
    let mut session = Session {
        shared: &shared,
        stream_errors,
        device: &output.device,
        logger,
    };
    let result = session.event_loop(&mut terminal, &mut app);
    ratatui::restore();

    shared.store(Params {
        gain: 0.0,
        ..app.params()
    });
    std::thread::sleep(FADE_OUT);
    result.map(|()| app.play_time)
}

/// Everything the event loop needs besides the terminal and app state.
struct Session<'a> {
    shared: &'a SharedParams,
    stream_errors: Receiver<String>,
    device: &'a str,
    logger: &'a mut Logger,
}

impl Session<'_> {
    /// Draws and handles input until the app quits or the audio stream fails.
    fn event_loop(&mut self, terminal: &mut DefaultTerminal, app: &mut App) -> Result<(), String> {
        let mut last_tick = Instant::now();
        while !app.should_quit {
            terminal
                .draw(|frame| ui::render(frame, app, self.device))
                .map_err(|error| format!("drawing interface: {error}; restart the terminal"))?;
            if let Ok(error) = self.stream_errors.try_recv() {
                return Err(format!(
                    "audio stream on `{}` failed: {error}; reconnect the device and restart",
                    self.device
                ));
            }
            self.handle_input(app)?;
            app.tick(last_tick.elapsed());
            last_tick = Instant::now();
            self.shared.store(app.params());
        }
        Ok(())
    }

    /// Waits up to one frame for a key press, applies it, and logs setting changes.
    fn handle_input(&mut self, app: &mut App) -> Result<(), String> {
        let read_error = |error| format!("reading keyboard: {error}; restart the terminal");
        if !event::poll(FRAME).map_err(read_error)? {
            return Ok(());
        }
        if let Event::Key(key) = event::read().map_err(read_error)?
            && key.kind != KeyEventKind::Release
        {
            let before = (app.settings, app.playing);
            app.handle_key(key);
            if (app.settings, app.playing) != before {
                let message = format!("changed: {:?}, playing {}", app.settings, app.playing);
                self.logger.log(Level::Debug, &message)?;
            }
        }
        Ok(())
    }
}
