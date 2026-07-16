//! Bounded conversion boundary between native input callbacks and ASR consumers.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::Arc;

use super::recognizer::SAMPLE_RATE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureConfigError {
    ZeroChannels,
    ZeroSampleRate,
    ZeroCapacity,
}

/// The callback-owned half of a fixed-capacity PCM queue.
pub struct PcmProducer {
    sender: SyncSender<f32>,
    channels: usize,
    source_rate: u32,
    channel_sum: f64,
    channel_offset: usize,
    resample_accumulator: u32,
    dropped_samples: Arc<AtomicU64>,
}

/// The consumer-owned half. Reading never occurs on the native audio thread.
pub struct PcmConsumer {
    receiver: Receiver<f32>,
    dropped_samples: Arc<AtomicU64>,
}

pub fn bounded_pcm_channel(
    channels: u16,
    source_rate: u32,
    capacity_samples: usize,
) -> Result<(PcmProducer, PcmConsumer), CaptureConfigError> {
    if channels == 0 {
        return Err(CaptureConfigError::ZeroChannels);
    }
    if source_rate == 0 {
        return Err(CaptureConfigError::ZeroSampleRate);
    }
    if capacity_samples == 0 {
        return Err(CaptureConfigError::ZeroCapacity);
    }
    let (sender, receiver) = mpsc::sync_channel(capacity_samples);
    let dropped_samples = Arc::new(AtomicU64::new(0));
    Ok((
        PcmProducer {
            sender,
            channels: usize::from(channels),
            source_rate,
            channel_sum: 0.0,
            channel_offset: 0,
            resample_accumulator: 0,
            dropped_samples: dropped_samples.clone(),
        },
        PcmConsumer {
            receiver,
            dropped_samples,
        },
    ))
}

impl PcmProducer {
    pub fn push_f32(&mut self, samples: &[f32]) {
        self.push(samples.iter().map(|sample| {
            if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        }));
    }

    pub fn push_i16(&mut self, samples: &[i16]) {
        self.push(samples.iter().map(|sample| f32::from(*sample) / 32_768.0));
    }

    pub fn push_u16(&mut self, samples: &[u16]) {
        self.push(
            samples
                .iter()
                .map(|sample| (f32::from(*sample) - 32_768.0) / 32_768.0),
        );
    }

    fn push(&mut self, samples: impl Iterator<Item = f32>) {
        for sample in samples {
            self.channel_sum += f64::from(sample);
            self.channel_offset += 1;
            if self.channel_offset != self.channels {
                continue;
            }
            let mono = (self.channel_sum / self.channels as f64) as f32;
            self.channel_sum = 0.0;
            self.channel_offset = 0;

            self.resample_accumulator += SAMPLE_RATE;
            while self.resample_accumulator >= self.source_rate {
                self.resample_accumulator -= self.source_rate;
                match self.sender.try_send(mono) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) => {
                        self.dropped_samples.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(TrySendError::Disconnected(_)) => return,
                }
            }
        }
    }
}

impl PcmConsumer {
    pub fn drain(&self) -> Vec<f32> {
        let mut samples = Vec::new();
        loop {
            match self.receiver.try_recv() {
                Ok(sample) => samples.push(sample),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return samples,
            }
        }
    }

    pub fn dropped_samples(&self) -> u64 {
        self.dropped_samples.load(Ordering::Relaxed)
    }
}
