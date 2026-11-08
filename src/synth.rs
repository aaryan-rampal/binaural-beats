//! Stereo sine synthesis, kept free of audio-device code so it can be tested directly.

use std::f64::consts::TAU;
use std::sync::atomic::{AtomicU32, Ordering};

/// Time constant for gliding frequency and gain to a new target, which prevents clicks.
const SMOOTHING_SECONDS: f64 = 0.05;

/// The values the audio thread renders: the UI writes them and the callback reads them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub carrier_hz: f32,
    pub beat_hz: f32,
    pub gain: f32,
}

impl Params {
    /// Left-ear frequency: the carrier shifted down by half the beat.
    pub fn left_hz(self) -> f32 {
        self.carrier_hz - self.beat_hz / 2.0
    }

    /// Right-ear frequency: the carrier shifted up by half the beat.
    pub fn right_hz(self) -> f32 {
        self.carrier_hz + self.beat_hz / 2.0
    }
}

/// Lock-free parameter store, so the real-time callback never waits on the UI thread.
pub struct SharedParams {
    carrier_bits: AtomicU32,
    beat_bits: AtomicU32,
    gain_bits: AtomicU32,
}

impl SharedParams {
    /// Creates the store holding `params`.
    pub fn new(params: Params) -> Self {
        Self {
            carrier_bits: AtomicU32::new(params.carrier_hz.to_bits()),
            beat_bits: AtomicU32::new(params.beat_hz.to_bits()),
            gain_bits: AtomicU32::new(params.gain.to_bits()),
        }
    }

    /// Publishes new targets. Fields update independently, which smoothing makes inaudible.
    pub fn store(&self, params: Params) {
        self.carrier_bits
            .store(params.carrier_hz.to_bits(), Ordering::Relaxed);
        self.beat_bits
            .store(params.beat_hz.to_bits(), Ordering::Relaxed);
        self.gain_bits
            .store(params.gain.to_bits(), Ordering::Relaxed);
    }

    /// Reads the current targets.
    pub fn load(&self) -> Params {
        Params {
            carrier_hz: f32::from_bits(self.carrier_bits.load(Ordering::Relaxed)),
            beat_hz: f32::from_bits(self.beat_bits.load(Ordering::Relaxed)),
            gain: f32::from_bits(self.gain_bits.load(Ordering::Relaxed)),
        }
    }
}

/// Phase-continuous stereo oscillator pair with smoothed parameters.
pub struct Synth {
    sample_rate: f64,
    smoothing: f64,
    left_phase: f64,
    right_phase: f64,
    carrier_hz: f64,
    beat_hz: f64,
    gain: f64,
}

impl Synth {
    /// Creates a synth at the frequencies in `initial`, starting silent so it fades in.
    pub fn new(sample_rate: f64, initial: Params) -> Self {
        Self {
            sample_rate,
            smoothing: 1.0 - (-1.0 / (SMOOTHING_SECONDS * sample_rate)).exp(),
            left_phase: 0.0,
            right_phase: 0.0,
            carrier_hz: f64::from(initial.carrier_hz),
            beat_hz: f64::from(initial.beat_hz),
            gain: 0.0,
        }
    }

    /// Renders one `[left, right]` frame, moving one step toward `target`.
    pub fn next_frame(&mut self, target: Params) -> [f32; 2] {
        self.carrier_hz += (f64::from(target.carrier_hz) - self.carrier_hz) * self.smoothing;
        self.beat_hz += (f64::from(target.beat_hz) - self.beat_hz) * self.smoothing;
        self.gain += (f64::from(target.gain) - self.gain) * self.smoothing;

        let left = (TAU * self.left_phase).sin() * self.gain;
        let right = (TAU * self.right_phase).sin() * self.gain;

        let left_hz = self.carrier_hz - self.beat_hz / 2.0;
        let right_hz = self.carrier_hz + self.beat_hz / 2.0;
        self.left_phase = (self.left_phase + left_hz / self.sample_rate).fract();
        self.right_phase = (self.right_phase + right_hz / self.sample_rate).fract();

        [left as f32, right as f32]
    }
}

#[cfg(test)]
mod tests {
    use crate::synth::{Params, SharedParams, Synth};

    const SAMPLE_RATE: f64 = 48_000.0;
    const PARAMS: Params = Params {
        carrier_hz: 200.0,
        beat_hz: 10.0,
        gain: 0.5,
    };

    /// Renders `seconds` of audio into separate left and right buffers.
    fn render(synth: &mut Synth, target: Params, seconds: f64) -> (Vec<f32>, Vec<f32>) {
        let frames = (seconds * SAMPLE_RATE) as usize;
        (0..frames)
            .map(|_| synth.next_frame(target))
            .map(|[l, r]| (l, r))
            .unzip()
    }

    /// Counts negative-to-positive transitions, which equals cycles for a pure sine.
    fn rising_crossings(samples: &[f32]) -> usize {
        samples
            .windows(2)
            .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0)
            .count()
    }

    #[test]
    fn each_ear_gets_its_own_frequency_and_they_differ_by_the_beat() {
        let mut synth = Synth::new(SAMPLE_RATE, PARAMS);
        let (left, right) = render(&mut synth, PARAMS, 10.0);
        assert!(
            rising_crossings(&left).abs_diff(1950) <= 1,
            "left: {}",
            rising_crossings(&left)
        );
        assert!(
            rising_crossings(&right).abs_diff(2050) <= 1,
            "right: {}",
            rising_crossings(&right)
        );
    }

    #[test]
    fn output_never_exceeds_the_target_gain() {
        let mut synth = Synth::new(SAMPLE_RATE, PARAMS);
        let (left, right) = render(&mut synth, PARAMS, 2.0);
        let peak = left
            .iter()
            .chain(&right)
            .fold(0.0_f32, |max, sample| max.max(sample.abs()));
        assert!(peak <= PARAMS.gain + 1e-6, "peak {peak}");
        assert!(peak > PARAMS.gain * 0.99, "peak {peak}");
    }

    #[test]
    fn a_large_parameter_jump_does_not_click() {
        let mut synth = Synth::new(SAMPLE_RATE, PARAMS);
        let jump = Params {
            carrier_hz: 800.0,
            beat_hz: 40.0,
            gain: 1.0,
        };
        render(&mut synth, PARAMS, 1.0);
        let (left, right) = render(&mut synth, jump, 1.0);
        // A full-scale sine at the highest ear frequency, 820 Hz, moves at most 0.108 per sample.
        for channel in [&left, &right] {
            let largest = channel
                .windows(2)
                .map(|pair| (pair[1] - pair[0]).abs())
                .fold(0.0_f32, f32::max);
            assert!(largest < 0.11, "largest step {largest}");
        }
    }

    #[test]
    fn zero_gain_fades_to_silence() {
        let mut synth = Synth::new(SAMPLE_RATE, PARAMS);
        render(&mut synth, PARAMS, 1.0);
        render(
            &mut synth,
            Params {
                gain: 0.0,
                ..PARAMS
            },
            0.5,
        );
        let (left, _) = render(
            &mut synth,
            Params {
                gain: 0.0,
                ..PARAMS
            },
            0.1,
        );
        assert!(left.iter().all(|sample| sample.abs() < 1e-3));
    }

    #[test]
    fn synth_starts_silent() {
        let mut synth = Synth::new(SAMPLE_RATE, PARAMS);
        assert_eq!(synth.next_frame(PARAMS), [0.0, 0.0]);
    }

    #[test]
    fn shared_params_round_trip() {
        let shared = SharedParams::new(PARAMS);
        assert_eq!(shared.load(), PARAMS);
        let changed = Params {
            carrier_hz: 432.0,
            beat_hz: 7.83,
            gain: 0.1,
        };
        shared.store(changed);
        assert_eq!(shared.load(), changed);
    }
}
