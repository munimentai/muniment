# Handy voice architecture reference

Handy is an offline speech-to-text desktop app. It uses Tauri with a Rust
backend and a React and TypeScript frontend.

This reference describes Handy at commit
[`76736d5`](https://github.com/cjpais/Handy/tree/76736d5ac7bd6d4a6328a6ab392748aa35f5a12b).
The status column compares Handy with the current Muniment voice feature.

## Component map

| Component | Handy crate or module | Handy pattern | Muniment status |
| --- | --- | --- | --- |
| Audio capture | [`cpal` in `audio_toolkit::audio::recorder`](https://github.com/cjpais/Handy/blob/76736d5ac7bd6d4a6328a6ab392748aa35f5a12b/src-tauri/src/audio_toolkit/audio/recorder.rs) | The recorder opens the input device with `cpal`. It converts the stream to 16 kHz mono frames. | **Covered.** `src-tauri/src/voice_capture.rs` also uses `cpal` and supplies 16 kHz mono PCM. |
| Voice activity detection | [Silero through `vad-rs` in `audio_toolkit::vad::silero`](https://github.com/cjpais/Handy/blob/76736d5ac7bd6d4a6328a6ab392748aa35f5a12b/src-tauri/src/audio_toolkit/vad/silero.rs) | The module sends 30 ms frames to a Silero model. It compares each speech probability with a configured threshold. | **Different approach.** Muniment uses Silero through the pinned sherpa-onnx API in ADR 0004. |
| Global hotkey and push-to-talk | [`rdev`, with `shortcut` and `transcription_coordinator`](https://github.com/cjpais/Handy/blob/76736d5ac7bd6d4a6328a6ab392748aa35f5a12b/src-tauri/src/shortcut/mod.rs) | `rdev` remains a declared core library, while the current shortcut module selects a Tauri or `handy-keys` backend. The coordinator uses press and release events for push-to-talk. | **Different approach.** Muniment uses `tauri-plugin-global-shortcut` and its voice gesture controller for hold-to-talk. |
| Parakeet inference | [`transcribe-rs` in `managers::transcription`](https://github.com/cjpais/Handy/blob/76736d5ac7bd6d4a6328a6ab392748aa35f5a12b/src-tauri/src/managers/transcription.rs) | The manager loads an INT8 Parakeet model and runs local ONNX inference. Handy describes this path as CPU-optimized. | **Different approach.** Muniment runs its pinned Parakeet INT8 model through sherpa-onnx on the CPU. |
| Whisper GGML inference | [`transcribe-cpp` in `managers::transcription`](https://github.com/cjpais/Handy/blob/76736d5ac7bd6d4a6328a6ab392748aa35f5a12b/src-tauri/src/managers/transcription.rs) | The manager loads Whisper-family GGML or GGUF models into reusable `transcribe-cpp` sessions. | **Not covered.** ADR 0004 defers whisper.cpp as a fallback and does not include a Whisper runtime. |
| Tray integration | [Tauri tray API in `tray` and `lib`](https://github.com/cjpais/Handy/blob/76736d5ac7bd6d4a6328a6ab392748aa35f5a12b/src-tauri/src/tray.rs) | Handy changes the tray icon for idle, recording, and transcription states. Its tray menu also exposes voice actions. | **Not covered.** The desktop specification defines a general tray, but the current native app has no voice tray integration. |
| Text injection into the focused app | [`clipboard` with `enigo` and native Linux tools](https://github.com/cjpais/Handy/blob/76736d5ac7bd6d4a6328a6ab392748aa35f5a12b/src-tauri/src/clipboard.rs) | The module types text directly or pastes through the clipboard. It uses native input tools on Linux and falls back to `enigo`. | **Not covered.** Muniment sends transcripts to its own composer and does not inject text into another focused app. |

## Known issues to avoid

- Whisper models crash on some Windows and Linux configurations.
- Wayland text injection needs `wtype` or `dotool`. X11 text injection uses
  `xdotool`.
- On X11, the recording overlay can take focus and interfere with pasting.

Handy records these limits in its
[known issues and Linux notes](https://github.com/cjpais/Handy/blob/76736d5ac7bd6d4a6328a6ab392748aa35f5a12b/README.md#known-issues--current-limitations).

## License and use

Handy releases its source under the
[MIT License](https://github.com/cjpais/Handy/blob/76736d5ac7bd6d4a6328a6ab392748aa35f5a12b/LICENSE).
Its README places separate limits on the Handy name, logo, icon, and brand
assets. These branding limits apply to forks and other redistributions. Forks
must use their own branding and must not imply an affiliation.

This document uses Handy only as a pattern reference. Copying code from Handy
is out of scope for this ticket.

## Sources

- [Handy README and architecture](https://github.com/cjpais/Handy/blob/76736d5ac7bd6d4a6328a6ab392748aa35f5a12b/README.md#architecture)
- [Handy Rust dependencies](https://github.com/cjpais/Handy/blob/76736d5ac7bd6d4a6328a6ab392748aa35f5a12b/src-tauri/Cargo.toml)
- [Handy MIT License](https://github.com/cjpais/Handy/blob/76736d5ac7bd6d4a6328a6ab392748aa35f5a12b/LICENSE)
- [Muniment desktop ASR decision](../decisions/0004-desktop-asr-runtime.md)
- [Muniment desktop specification](../spec/02-desktop-app.md)
