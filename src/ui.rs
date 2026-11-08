//! Draws the interface.

use std::f64::consts::PI;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{self, Canvas};
use ratatui::widgets::{Block, BorderType, Paragraph};

use crate::app::{App, BANDS, Control};

const ACCENT: Color = Color::Cyan;
const LABEL_WIDTH: usize = 8;
const VALUE_WIDTH: usize = 10;
/// Width of the selection marker plus the gaps around the slider.
const ROW_PADDING: usize = 4;

/// Draws one frame showing `app`, with `device` as the output name.
pub fn render(frame: &mut Frame, app: &App, device: &str) {
    let [status, controls, scope, help] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(5),
        Constraint::Min(3),
        Constraint::Length(4),
    ])
    .areas(frame.area());

    render_status(frame, status, app, device);
    render_controls(frame, controls, app);
    render_scope(frame, scope, app);
    render_help(frame, help);
}

/// A rounded panel with a title.
fn panel(title: String) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
}

/// Play state, band, per-ear frequencies and play time.
fn render_status(frame: &mut Frame, area: Rect, app: &App, device: &str) {
    let params = app.params();
    let total = app.play_time.as_secs();
    let clock = format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        total / 60 % 60,
        total % 60
    );
    let (state, color) = if app.playing {
        ("▶ playing", Color::Green)
    } else {
        ("⏸ paused", Color::Yellow)
    };
    let line = Line::from(vec![
        Span::styled(state, Style::new().fg(color).add_modifier(Modifier::BOLD)),
        Span::raw("   "),
        Span::styled(
            app.band_name(),
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "   L {:.1} Hz   R {:.1} Hz   {clock}",
            params.left_hz(),
            params.right_hz()
        )),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(panel(format!(" binaural · {device} "))),
        area,
    );
}

/// One slider row per control.
fn render_controls(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" controls ".to_string());
    let width = usize::from(block.inner(area).width);
    let lines: Vec<Line> = Control::ALL
        .iter()
        .enumerate()
        .map(|(row, control)| control_line(app, *control, row == app.selected, width))
        .collect();
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// Builds a slider row that fits in `width` columns.
fn control_line(app: &App, control: Control, selected: bool, width: usize) -> Line<'static> {
    let readout = app.readout(control);
    let slider_width = width.saturating_sub(LABEL_WIDTH + VALUE_WIDTH + ROW_PADDING);
    let filled = (readout.fraction * slider_width as f32).round() as usize;
    let style = if selected {
        Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    Line::from(vec![
        Span::styled(if selected { "▸ " } else { "  " }, style),
        Span::styled(format!("{:<LABEL_WIDTH$}", readout.label), style),
        Span::styled("━".repeat(filled), style.fg(ACCENT)),
        Span::styled(
            "─".repeat(slider_width - filled),
            Style::new().fg(Color::DarkGray),
        ),
        Span::styled(format!("  {:>VALUE_WIDTH$}", readout.value), style),
    ])
}

/// The beat envelope scrolling at real time, so higher beats show more pulses across the panel.
fn render_scope(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(format!(
        " beat envelope, last 1 s: {:.1} pulses ",
        app.settings.beat_hz
    ));
    let points = envelope_points(app, usize::from(block.inner(area).width) * 2);
    let color = if app.params().gain > 0.0 {
        ACCENT
    } else {
        Color::DarkGray
    };
    let canvas = Canvas::default()
        .block(block)
        .marker(Marker::Braille)
        .x_bounds([-1.0, 0.0])
        .y_bounds([0.0, 1.0])
        .paint(|context| {
            for pair in points.windows(2) {
                let ((x1, y1), (x2, y2)) = (pair[0], pair[1]);
                context.draw(&canvas::Line::new(x1, y1, x2, y2, color));
            }
        });
    frame.render_widget(canvas, area);
}

/// Samples `|cos(pi * phase)|` over the last second as `(-seconds_ago, height)` pairs.
///
/// Height shows the shape rather than loudness, so the pulses stay readable at low volume.
fn envelope_points(app: &App, count: usize) -> Vec<(f64, f64)> {
    let audible = app.params().gain > 0.0;
    let beat = f64::from(app.settings.beat_hz);
    (0..count)
        .map(|index| {
            let seconds_ago = index as f64 / count as f64;
            let phase = app.beat_phase - beat * seconds_ago;
            let height = if audible {
                (PI * phase).cos().abs()
            } else {
                0.0
            };
            (-seconds_ago, height)
        })
        .collect()
}

/// Key reference and the headphone reminder.
fn render_help(frame: &mut Frame, area: Rect) {
    let presets: Vec<String> = BANDS
        .iter()
        .enumerate()
        .map(|(i, band)| format!("{} {}", i + 1, band.name))
        .collect();
    let lines = vec![
        Line::raw("↑↓ select   ←→ adjust   space pause   q quit"),
        Line::raw(format!("{}   (headphones required)", presets.join("  "))),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::new().fg(Color::Gray))
            .block(panel(" keys ".into())),
        area,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::app::{App, Settings};
    use crate::ui::{envelope_points, render};

    /// Renders `app` to a test terminal and returns its text.
    fn screen(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render(frame, app, "Test Phones"))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn shows_device_band_and_each_ear() {
        let text = screen(&App::new(Settings::default()), 80, 24);
        for expected in [
            "Test Phones",
            "playing",
            "alpha",
            "L 145.0 Hz",
            "R 155.0 Hz",
            "10.0 Hz",
            "25%",
        ] {
            assert!(text.contains(expected), "missing {expected}");
        }
    }

    #[test]
    fn shows_pause_state() {
        let mut app = App::new(Settings::default());
        app.playing = false;
        assert!(screen(&app, 80, 24).contains("paused"));
    }

    #[test]
    fn tiny_terminals_do_not_panic() {
        let app = App::new(Settings::default());
        for (width, height) in [(1, 1), (10, 3), (20, 5), (30, 40)] {
            screen(&app, width, height);
        }
    }

    #[test]
    fn envelope_shows_one_pulse_per_beat_cycle() {
        let app = App::new(Settings {
            beat_hz: 10.0,
            ..Settings::default()
        });
        let heights: Vec<f64> = envelope_points(&app, 1000).iter().map(|p| p.1).collect();
        let peaks = heights
            .windows(3)
            .filter(|w| w[1] > w[0] && w[1] >= w[2] && w[1] > 0.99)
            .count();
        assert!((9..=11).contains(&peaks), "peaks: {peaks}");
    }

    #[test]
    fn envelope_is_flat_while_paused() {
        let mut app = App::new(Settings::default());
        app.playing = false;
        assert!(
            envelope_points(&app, 100)
                .iter()
                .all(|point| point.1 == 0.0)
        );
    }
}
