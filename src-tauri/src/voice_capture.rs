use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

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

pub struct CapturedPcm {
    pub samples: Vec<f32>,
    /// Samples were lost after this batch. Callers must break streaming state
    /// before processing the next batch.
    pub discontinuity_after: bool,
}

pub struct VoiceCaptureState {
    active: Mutex<Option<ActiveCapture>>,
    runtime_failed: Arc<AtomicBool>,
}

struct ActiveCapture {
    stop: SyncSender<()>,
    thread: Option<JoinHandle<()>>,
    consumer: PcmConsumer,
}

impl Drop for ActiveCapture {
    fn drop(&mut self) {
        let _ = self.stop.try_send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl VoiceCaptureState {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(None),
            runtime_failed: Arc::new(AtomicBool::new(false)),
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
        self.runtime_failed.store(false, Ordering::Release);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (stop_tx, stop_rx) = mpsc::sync_channel(1);
        let runtime_failed = self.runtime_failed.clone();
        let capture_thread = thread::Builder::new()
            .name("voice-capture".into())
            .spawn(move || {
                let result = Self::open_stream(runtime_failed).and_then(|(stream, consumer)| {
                    stream
                        .play()
                        .map_err(|_| VoiceCaptureError::StreamBuildFailed)?;
                    Ok((stream, consumer))
                });
                match result {
                    Ok((stream, consumer)) => {
                        if ready_tx.send(Ok(consumer)).is_ok() {
                            let _ = stop_rx.recv();
                        }
                        drop(stream);
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                    }
                }
            })
            .map_err(|_| VoiceCaptureError::StreamBuildFailed)?;
        let consumer = match ready_rx.recv() {
            Ok(Ok(consumer)) => consumer,
            Ok(Err(error)) => {
                let _ = capture_thread.join();
                return Err(error);
            }
            Err(_) => {
                let _ = capture_thread.join();
                return Err(VoiceCaptureError::StreamBuildFailed);
            }
        };
        *active = Some(ActiveCapture {
            stop: stop_tx,
            thread: Some(capture_thread),
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

    /// Stops capture and returns every sample queued before the stream closed.
    pub fn stop_and_take_samples(&self) -> Result<CapturedPcm, VoiceCaptureError> {
        let active = self
            .active
            .lock()
            .map_err(|_| VoiceCaptureError::StreamBuildFailed)?
            .take();
        Ok(active.map_or_else(
            || CapturedPcm {
                samples: Vec::new(),
                discontinuity_after: false,
            },
            |mut capture| {
                let _ = capture.stop.try_send(());
                if let Some(thread) = capture.thread.take() {
                    let _ = thread.join();
                }
                drain_capture(&capture.consumer)
            },
        ))
    }

    pub fn take_samples(&self) -> Result<CapturedPcm, VoiceCaptureError> {
        if self.runtime_failed.swap(false, Ordering::AcqRel) {
            return Err(VoiceCaptureError::RuntimeStreamFailed);
        }
        let active = self
            .active
            .lock()
            .map_err(|_| VoiceCaptureError::RuntimeStreamFailed)?;
        Ok(active.as_ref().map_or_else(
            || CapturedPcm {
                samples: Vec::new(),
                discontinuity_after: false,
            },
            |capture| drain_capture(&capture.consumer),
        ))
    }

    fn open_stream(
        runtime_failed: Arc<AtomicBool>,
    ) -> Result<(cpal::Stream, PcmConsumer), VoiceCaptureError> {
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
        let stream = Self::build_stream(
            &device,
            &config,
            supported.sample_format(),
            producer,
            runtime_failed,
        )?;
        Ok((stream, consumer))
    }

    fn build_stream(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        format: cpal::SampleFormat,
        producer: PcmProducer,
        runtime_failed: Arc<AtomicBool>,
    ) -> Result<cpal::Stream, VoiceCaptureError> {
        let on_error = move |_error| {
            runtime_failed.store(true, Ordering::Release);
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

fn drain_capture(consumer: &PcmConsumer) -> CapturedPcm {
    let samples = consumer.drain();
    // Reset only after draining: queued audio precedes the reported gap. A
    // concurrent drop is conservatively assigned after this batch as well.
    let discontinuity_after = consumer.take_dropped_samples() != 0;
    CapturedPcm {
        samples,
        discontinuity_after,
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

    impl Drop for FakeStream {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn state_with_fake_stream(drops: Arc<AtomicUsize>) -> VoiceCaptureState {
        let state = VoiceCaptureState::new();
        let (_, consumer) = bounded_pcm_channel(1, 16_000, 1).unwrap();
        let (stop, stopped) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            let _stream = FakeStream(drops);
            let _ = stopped.recv();
        });
        *state.active.lock().unwrap() = Some(ActiveCapture {
            stop,
            thread: Some(thread),
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

    #[test]
    fn runtime_failure_is_reported_once_then_cleared() {
        let state = VoiceCaptureState::new();
        state.runtime_failed.store(true, Ordering::Release);
        assert_eq!(
            state.take_samples().map(|batch| batch.samples),
            Err(VoiceCaptureError::RuntimeStreamFailed)
        );
        assert_eq!(state.take_samples().unwrap().samples, Vec::<f32>::new());
    }
}
