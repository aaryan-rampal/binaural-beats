//! Connects the synth to the default output device.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, SampleFormat};

use crate::synth::{SharedParams, Synth};

/// Something the stream reported from the audio thread.
#[derive(Debug)]
pub struct StreamEvent {
    pub message: String,
    /// Whether the stream stopped. Glitches and automatic reroutes leave it playing.
    pub fatal: bool,
    /// The stream moved to a new default output; read its name with [`default_output_name`].
    pub rerouted: bool,
}

impl StreamEvent {
    /// Classifies a cpal stream error.
    fn from_error(error: &cpal::Error) -> Self {
        let kind = error.kind();
        let rerouted = kind == ErrorKind::DeviceChanged;
        Self {
            message: error.to_string(),
            fatal: !rerouted && kind != ErrorKind::Xrun,
            rerouted,
        }
    }
}

/// Name of the current default output device, if there is one.
pub fn default_output_name() -> Option<String> {
    let device = cpal::default_host().default_output_device()?;
    Some(device_name(&device))
}

/// A device's display name, or "unknown" when the system will not say.
fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map_or_else(|_| "unknown".into(), |d| d.name().to_string())
}

/// A running output stream. Audio stops when this is dropped.
pub struct AudioOutput {
    _stream: cpal::Stream,
    pub device: String,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Opens the default output device and starts rendering from `shared`.
///
/// Stream events after startup go to `on_event`, which runs on the audio thread. The stream
/// follows the system default output, so switching outputs reroutes it rather than failing.
pub fn start(
    shared: Arc<SharedParams>,
    mut on_event: impl FnMut(StreamEvent) + Send + 'static,
) -> Result<AudioOutput, String> {
    let device = cpal::default_host().default_output_device().ok_or(
        "opening audio output: no default output device; choose one in System Settings > Sound",
    )?;
    let name = device_name(&device);
    let supported = device.default_output_config().map_err(|error| {
        format!("reading config of output `{name}`: {error}; try another output device")
    })?;

    // Binaural beats depend on each ear getting its own tone, so mono output cannot work.
    if supported.channels() < 2 {
        return Err(format!(
            "output `{name}` has {} channel(s); binaural beats need stereo, so switch to \
             headphones in System Settings > Sound",
            supported.channels()
        ));
    }
    if supported.sample_format() != SampleFormat::F32 {
        return Err(format!(
            "output `{name}` uses sample format {}; this tool only renders f32, so try another \
             output device",
            supported.sample_format()
        ));
    }

    let channels = supported.channels();
    let sample_rate = supported.sample_rate();
    let mut synth = Synth::new(f64::from(sample_rate), shared.load());
    let render = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        let target = shared.load();
        for frame in data.chunks_mut(usize::from(channels)) {
            let [left, right] = synth.next_frame(target);
            frame[0] = left;
            frame[1] = right;
            frame[2..].fill(0.0);
        }
    };
    let report = move |error: cpal::Error| on_event(StreamEvent::from_error(&error));

    let stream = device
        .build_output_stream::<f32, _, _>(supported.config(), render, report, None)
        .map_err(|error| format!("opening stream on `{name}`: {error}; try another output"))?;
    stream
        .play()
        .map_err(|error| format!("starting stream on `{name}`: {error}; try another output"))?;

    Ok(AudioOutput {
        _stream: stream,
        device: name,
        sample_rate,
        channels,
    })
}

#[cfg(test)]
mod tests {
    use cpal::ErrorKind;

    use crate::audio::StreamEvent;

    /// Classifies a bare error of `kind`.
    fn event(kind: ErrorKind) -> StreamEvent {
        StreamEvent::from_error(&cpal::Error::from(kind))
    }

    #[test]
    fn glitches_and_reroutes_keep_playing_but_disconnects_stop() {
        assert!(!event(ErrorKind::Xrun).fatal);
        let reroute = event(ErrorKind::DeviceChanged);
        assert!(!reroute.fatal && reroute.rerouted);
        assert!(event(ErrorKind::DeviceNotAvailable).fatal);
        assert!(event(ErrorKind::StreamInvalidated).fatal);
    }
}
