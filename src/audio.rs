//! Connects the synth to the default output device.

use std::sync::Arc;
use std::sync::mpsc::Sender;

use cpal::SampleFormat;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::synth::{SharedParams, Synth};

/// A running output stream. Audio stops when this is dropped.
pub struct AudioOutput {
    _stream: cpal::Stream,
    pub device: String,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Opens the default output device and starts rendering from `shared`.
///
/// Stream failures after startup arrive on `errors`, because they happen on the audio thread.
pub fn start(shared: Arc<SharedParams>, errors: Sender<String>) -> Result<AudioOutput, String> {
    let device = cpal::default_host().default_output_device().ok_or(
        "opening audio output: no default output device; choose one in System Settings > Sound",
    )?;
    let name = device
        .description()
        .map_or_else(|_| "unknown".into(), |d| d.name().to_string());
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
    let report = move |error: cpal::Error| {
        // The receiver is gone only during shutdown, when a stream error no longer matters.
        let _ = errors.send(error.to_string());
    };

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
