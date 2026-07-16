use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use muniment_core::asr::capture::{bounded_pcm_channel, PcmConsumer, PcmProducer};

const QUEUE_CAPACITY_SAMPLES: usize = 16_000 * 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceCaptureError {
    NoInputDevice,
    UnsupportedConfig,
    StreamBuildFailed,
    RuntimeStreamFailed,
    AlreadyRunning,
}

pub struct VoiceCaptureState {
    active: Mutex<Option<ActiveCapture>>,
    runtime_error: Arc<Mutex<Option<VoiceCaptureError>>>,
}

struct ActiveCapture {
    _stream: Box<dyn OwnedStream>,
    consumer: PcmConsumer,
}

trait OwnedStream: Send {}

impl OwnedStream for cpal::Stream {}

impl VoiceCaptureState {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(None),
            runtime_error: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start(&self) -> Result<(), VoiceCaptureError> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| VoiceCaptureError::StreamBuildFailed)?;
        if active.is_some() {
            return Err(VoiceCaptureError::AlreadyRunning);
        }
        *self
            .runtime_error
            .lock()
            .map_err(|_| VoiceCaptureError::StreamBuildFailed)? = None;
        let device = cpal::default_host()
            .default_input_device()
            .ok_or(VoiceCaptureError::NoInputDevice)?;
        let supported = device
            .default_input_config()
            .map_err(|_| VoiceCaptureError::UnsupportedConfig)?;
        let config = supported.config();
        let (producer, consumer) = bounded_pcm_channel(
            config.channels,
            config.sample_rate.0,
            QUEUE_CAPACITY_SAMPLES,
        )
        .map_err(|_| VoiceCaptureError::UnsupportedConfig)?;
        let stream = self.build_stream(&device, &config, supported.sample_format(), producer)?;
        stream
            .play()
            .map_err(|_| VoiceCaptureError::StreamBuildFailed)?;
        *active = Some(ActiveCapture {
            _stream: Box::new(stream),
            consumer,
        });
        Ok(())
    }

    /// Stops capture. Stopping an idle capture is intentionally successful.
    pub fn stop(&self) -> Result<(), VoiceCaptureError> {
        self.active
            .lock()
            .map_err(|_| VoiceCaptureError::StreamBuildFailed)?
            .take();
        Ok(())
    }

    pub fn take_samples(&self) -> Result<Vec<f32>, VoiceCaptureError> {
        if let Some(error) = self
            .runtime_error
            .lock()
            .map_err(|_| VoiceCaptureError::RuntimeStreamFailed)?
            .take()
        {
            return Err(error);
        }
        let active = self
            .active
            .lock()
            .map_err(|_| VoiceCaptureError::RuntimeStreamFailed)?;
        Ok(active
            .as_ref()
            .map_or_else(Vec::new, |capture| capture.consumer.drain()))
    }

    fn build_stream(
        &self,
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        format: cpal::SampleFormat,
        producer: PcmProducer,
    ) -> Result<cpal::Stream, VoiceCaptureError> {
        let runtime_error = self.runtime_error.clone();
        let on_error = move |_error| {
            if let Ok(mut state) = runtime_error.try_lock() {
                *state = Some(VoiceCaptureError::RuntimeStreamFailed);
            }
        };
        match format {
            cpal::SampleFormat::F32 => {
                let mut producer = producer;
                device.build_input_stream(
                    config,
                    move |data, _| producer.push_f32(data),
                    on_error,
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let mut producer = producer;
                device.build_input_stream(
                    config,
                    move |data, _| producer.push_i16(data),
                    on_error,
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let mut producer = producer;
                device.build_input_stream(
                    config,
                    move |data, _| producer.push_u16(data),
                    on_error,
                    None,
                )
            }
            _ => return Err(VoiceCaptureError::UnsupportedConfig),
        }
        .map_err(|_| VoiceCaptureError::StreamBuildFailed)
    }
}

impl Drop for VoiceCaptureState {
    fn drop(&mut self) {
        if let Ok(active) = self.active.get_mut() {
            active.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeStream(Arc<AtomicUsize>);

    impl OwnedStream for FakeStream {}

    impl Drop for FakeStream {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn state_with_fake_stream(drops: Arc<AtomicUsize>) -> VoiceCaptureState {
        let state = VoiceCaptureState::new();
        let (_, consumer) = bounded_pcm_channel(1, 16_000, 1).unwrap();
        *state.active.lock().unwrap() = Some(ActiveCapture {
            _stream: Box::new(FakeStream(drops)),
            consumer,
        });
        state
    }

    #[test]
    fn stop_is_idempotent_and_releases_the_owned_stream() {
        let drops = Arc::new(AtomicUsize::new(0));
        let state = state_with_fake_stream(drops.clone());
        assert_eq!(state.stop(), Ok(()));
        assert_eq!(state.stop(), Ok(()));
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn dropping_state_releases_the_owned_stream() {
        let drops = Arc::new(AtomicUsize::new(0));
        drop(state_with_fake_stream(drops.clone()));
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }
}
