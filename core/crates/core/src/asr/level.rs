//! Voice loudness of the capture, for a host's live display of the speaker's
//! voice. The meter runs on the native audio callback, so it holds fixed state
//! and never allocates, and the slot hands the latest level to a reader
//! without a lock.

use std::sync::atomic::{AtomicU64, Ordering};

/// Levels the meter reports each second.
pub const VOICE_LEVEL_RATE: u32 = 30;

const BAND_CENTER_HZ: f64 = 900.0;
const BAND_Q: f64 = 0.5;
const FLOOR_DB: f64 = -52.0;
const RANGE_DB: f64 = 34.0;

/// Maps the RMS of the voice band to a loudness from 0 at -52 dBFS to 1 at
/// -18 dBFS.
pub fn voice_loudness(rms: f64) -> f32 {
    let db = 20.0 * (rms + 1e-6).log10();
    ((db - FLOOR_DB) / RANGE_DB).clamp(0.0, 1.0) as f32
}

/// A band-pass biquad with 0 dB gain at its center, so speech counts and
/// rumble and hiss do not.
#[derive(Debug, Clone)]
struct BandPass {
    b0: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl BandPass {
    fn new(sample_rate: u32) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * BAND_CENTER_HZ / f64::from(sample_rate);
        let alpha = w0.sin() / (2.0 * BAND_Q);
        let a0 = 1.0 + alpha;
        Self {
            b0: alpha / a0,
            b2: -alpha / a0,
            a1: -2.0 * w0.cos() / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, sample: f64) -> f64 {
        let output = self.b0 * sample + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = sample;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }
}

/// Measures the voice band of mono PCM in windows of one level each.
#[derive(Debug, Clone)]
pub struct VoiceLevelMeter {
    filter: BandPass,
    window: usize,
    sum: f64,
    count: usize,
}

impl VoiceLevelMeter {
    /// A meter for mono PCM at `sample_rate`, reporting [`VOICE_LEVEL_RATE`]
    /// levels a second.
    pub fn new(sample_rate: u32) -> Self {
        Self {
            filter: BandPass::new(sample_rate.max(1)),
            window: (sample_rate / VOICE_LEVEL_RATE).max(1) as usize,
            sum: 0.0,
            count: 0,
        }
    }

    /// Consumes one sample and returns the loudness of the window it ends.
    pub fn push(&mut self, sample: f32) -> Option<f32> {
        let filtered = self.filter.process(f64::from(sample));
        self.sum += filtered * filtered;
        self.count += 1;
        if self.count < self.window {
            return None;
        }
        let rms = (self.sum / self.count as f64).sqrt();
        self.sum = 0.0;
        self.count = 0;
        Some(voice_loudness(rms))
    }
}

/// One level the slot handed out. `sequence` grows with each stored level, so
/// a reader tells a new level from one it already read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceLevelFrame {
    pub sequence: u32,
    pub level: f32,
}

/// The latest level, shared between the audio callback and a reader.
#[derive(Debug, Default)]
pub struct VoiceLevelSlot(AtomicU64);

impl VoiceLevelSlot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn store(&self, level: f32) {
        let previous = self.0.load(Ordering::Acquire);
        let sequence = ((previous >> 32) as u32).wrapping_add(1).max(1);
        self.0.store(
            (u64::from(sequence) << 32) | u64::from(level.to_bits()),
            Ordering::Release,
        );
    }

    /// The latest level, or `None` before the first one.
    pub fn load(&self) -> Option<VoiceLevelFrame> {
        let value = self.0.load(Ordering::Acquire);
        let sequence = (value >> 32) as u32;
        (sequence != 0).then(|| VoiceLevelFrame {
            sequence,
            level: f32::from_bits(value as u32),
        })
    }

    /// Forgets the last level, for a new capture.
    pub fn reset(&self) {
        self.0.store(0, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000;

    fn tone(frequency: f64, amplitude: f64, seconds: f64) -> Vec<f32> {
        (0..(f64::from(RATE) * seconds) as usize)
            .map(|index| {
                let t = index as f64 / f64::from(RATE);
                (amplitude * (2.0 * std::f64::consts::PI * frequency * t).sin()) as f32
            })
            .collect()
    }

    fn levels(samples: &[f32]) -> Vec<f32> {
        let mut meter = VoiceLevelMeter::new(RATE);
        samples
            .iter()
            .filter_map(|sample| meter.push(*sample))
            .collect()
    }

    #[test]
    fn maps_decibels_to_loudness() {
        assert_eq!(voice_loudness(0.0), 0.0);
        assert_eq!(voice_loudness(1.0), 1.0);
        let at_minus_35 = 10f64.powf(-35.0 / 20.0);
        assert!((voice_loudness(at_minus_35) - 0.5).abs() < 0.01);
    }

    #[test]
    fn reports_thirty_levels_a_second() {
        assert_eq!(levels(&vec![0.0; RATE as usize]).len(), 30);
        assert_eq!(levels(&vec![0.0; 532]).len(), 0);
        assert_eq!(levels(&vec![0.0; 533]).len(), 1);
    }

    #[test]
    fn a_voice_band_tone_is_loud_and_silence_is_quiet() {
        let loud = levels(&tone(900.0, 0.5, 0.5));
        assert!(loud[3..].iter().all(|level| *level == 1.0));
        let moderate = levels(&tone(900.0, 0.01, 0.5));
        let settled = moderate.last().copied().unwrap();
        assert!((settled - 9.0 / 34.0).abs() < 0.03, "{settled}");
        assert!(levels(&vec![0.0; 8_000]).iter().all(|level| *level == 0.0));
    }

    #[test]
    fn rumble_outside_the_voice_band_counts_less() {
        let voice = *levels(&tone(900.0, 0.05, 0.5)).last().unwrap();
        let rumble = *levels(&tone(40.0, 0.05, 0.5)).last().unwrap();
        assert!(rumble + 0.2 < voice, "{rumble} {voice}");
    }

    #[test]
    fn the_slot_hands_out_each_level_once_by_sequence() {
        let slot = VoiceLevelSlot::new();
        assert_eq!(slot.load(), None);
        slot.store(0.25);
        let first = slot.load().unwrap();
        assert_eq!(first.level, 0.25);
        slot.store(0.75);
        let second = slot.load().unwrap();
        assert_eq!(second.level, 0.75);
        assert_ne!(first.sequence, second.sequence);
        slot.reset();
        assert_eq!(slot.load(), None);
    }
}
