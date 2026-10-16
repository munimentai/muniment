//! Dictation's sound around the microphone: the tones that mark each end of a
//! dictation and, on macOS, the system output muted while dictation listens.

use std::f32::consts::TAU;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

const TONE_LENGTH: Duration = Duration::from_millis(110);
const TONE_TAIL: Duration = Duration::from_millis(40);
const TONE_LEVEL: f32 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Start,
    Stop,
}

impl Tone {
    /// A rising pair of notes opens dictation and a falling pair closes it.
    fn pitch(self, progress: f32) -> f32 {
        let (first, second) = match self {
            Tone::Start => (660.0, 880.0),
            Tone::Stop => (880.0, 660.0),
        };
        if progress < 0.5 {
            first
        } else {
            second
        }
    }
}

/// One sample of the tone at `index` of `total` samples, or silence after it.
fn tone_sample(tone: Tone, index: usize, total: usize, phase: &mut f32, rate: f32) -> f32 {
    if index >= total {
        return 0.0;
    }
    let progress = index as f32 / total as f32;
    *phase = (*phase + TAU * tone.pitch(progress) / rate) % TAU;
    let envelope = (progress * 20.0).min(1.0) * (1.0 - progress);
    phase.sin() * envelope * TONE_LEVEL
}

/// Plays a tone on the default output and returns after it ends. A missing or
/// busy output device plays nothing.
pub fn play(tone: Tone) {
    let _ = try_play(tone);
}

fn try_play(tone: Tone) -> Option<()> {
    let device = cpal::default_host().default_output_device()?;
    let supported = device.default_output_config().ok()?;
    let config = supported.config();
    let rate = config.sample_rate as f32;
    let channels = usize::from(config.channels.max(1));
    let total = (rate * TONE_LENGTH.as_secs_f32()) as usize;
    let mut index = 0;
    let mut phase = 0.0;
    let mut next = move || {
        let sample = tone_sample(tone, index, total, &mut phase, rate);
        index += 1;
        sample
    };
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            config,
            move |data: &mut [f32], _| {
                for frame in data.chunks_mut(channels) {
                    frame.fill(next());
                }
            },
            |_| {},
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            config,
            move |data: &mut [i16], _| {
                for frame in data.chunks_mut(channels) {
                    frame.fill((next() * f32::from(i16::MAX)) as i16);
                }
            },
            |_| {},
            None,
        ),
        _ => return None,
    }
    .ok()?;
    stream.play().ok()?;
    std::thread::sleep(TONE_LENGTH + TONE_TAIL);
    Some(())
}

/// Mutes the system output until dropped. An output the user already muted
/// stays as it is, and the drop restores only a mute this guard set.
pub struct OutputMute {
    #[cfg(target_os = "macos")]
    device: Option<u32>,
}

impl OutputMute {
    pub fn engage() -> Self {
        Self {
            #[cfg(target_os = "macos")]
            device: core_audio::mute_default_output(),
        }
    }
}

impl Drop for OutputMute {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(device) = self.device {
            core_audio::unmute(device);
        }
    }
}

#[cfg(target_os = "macos")]
mod core_audio {
    use std::ffi::c_void;

    #[repr(C)]
    struct Address {
        selector: u32,
        scope: u32,
        element: u32,
    }

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyData(
            object: u32,
            address: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: *mut u32,
            data: *mut c_void,
        ) -> i32;
        fn AudioObjectSetPropertyData(
            object: u32,
            address: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: u32,
            data: *const c_void,
        ) -> i32;
    }

    const fn code(name: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*name)
    }

    const SYSTEM_OBJECT: u32 = 1;
    const DEFAULT_OUTPUT: Address = Address {
        selector: code(b"dOut"),
        scope: code(b"glob"),
        element: 0,
    };
    const OUTPUT_MUTE: Address = Address {
        selector: code(b"mute"),
        scope: code(b"outp"),
        element: 0,
    };
    const VALUE_SIZE: u32 = std::mem::size_of::<u32>() as u32;

    fn read(object: u32, address: &Address) -> Option<u32> {
        let mut value = 0u32;
        let mut size = VALUE_SIZE;
        // SAFETY: the address and the u32 out value outlive the call, and
        // `size` names the out value's size.
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                address,
                0,
                std::ptr::null(),
                &mut size,
                (&mut value as *mut u32).cast(),
            )
        };
        (status == 0).then_some(value)
    }

    fn write(object: u32, address: &Address, value: u32) -> bool {
        // SAFETY: the address and the u32 value outlive the call, and the
        // size argument names the value's size.
        let status = unsafe {
            AudioObjectSetPropertyData(
                object,
                address,
                0,
                std::ptr::null(),
                VALUE_SIZE,
                (&value as *const u32).cast(),
            )
        };
        status == 0
    }

    /// Mutes the default output and returns it, or None when the output is
    /// already muted or has no mute control.
    pub fn mute_default_output() -> Option<u32> {
        let device = read(SYSTEM_OBJECT, &DEFAULT_OUTPUT)?;
        if read(device, &OUTPUT_MUTE)? != 0 {
            return None;
        }
        write(device, &OUTPUT_MUTE, 1).then_some(device)
    }

    pub fn unmute(device: u32) {
        write(device, &OUTPUT_MUTE, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tones_rise_to_start_and_fall_to_stop() {
        assert!(Tone::Start.pitch(0.0) < Tone::Start.pitch(0.9));
        assert!(Tone::Stop.pitch(0.0) > Tone::Stop.pitch(0.9));
    }

    #[test]
    fn a_tone_stays_quiet_and_ends_in_silence() {
        let mut phase = 0.0;
        let total = 4_800;
        let samples: Vec<f32> = (0..total + 10)
            .map(|index| tone_sample(Tone::Start, index, total, &mut phase, 48_000.0))
            .collect();
        assert!(samples.iter().all(|sample| sample.abs() <= TONE_LEVEL));
        assert!(samples.iter().any(|sample| sample.abs() > TONE_LEVEL / 2.0));
        assert_eq!(samples[0], 0.0);
        assert!(samples[total..].iter().all(|sample| *sample == 0.0));
    }
}
