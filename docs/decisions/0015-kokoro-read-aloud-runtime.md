# 0015 — Pin the Kokoro read-aloud runtime and artifacts

- Status: accepted
- Date: 2026-07-22
- Context: ROADMAP Phase 3; harness-spec §6.7

## Context

Read-aloud must be reproducible, CPU-capable, and wholly on-device on every
supported desktop. Kokoro's weights and voice styles are separate from an
inference runtime and from the text-to-phoneme pipeline. Each boundary must be
fixed before acquisition, playback, or UI work begins. This is especially
important because upstream reports weaker results below 10–20 tokens and
rushing above 400 tokens.

## Decision

### Runtime and artifact identity

Muniment will run the Kokoro v1.0 INT8 graph in-process through the **ONNX
Runtime v1.20.1 C API**, CPU execution provider only, pinned to tag commit
`5c1b7ccbff7e5141c1da7a9d963d660e5741c319`. The Python `kokoro-onnx` package
is not shipped. Its MIT-licensed implementation at model-release commit
`6843c53fc280ab130b7a8d206ebd3407e094efdc` is the reference for graph inputs,
vocabulary, normalization, and output handling; the native implementation must
match versioned conformance fixtures derived from that revision.

The model and complete voice-style bundle are the following immutable GitHub
release assets. The release tag resolves to commit
`6843c53fc280ab130b7a8d206ebd3407e094efdc`; a mutable branch or a differently
named/encoded graph is not equivalent.

| Filename | Byte size | SHA-256 |
| --- | ---: | --- |
| `kokoro-v1.0.int8.onnx` | 92,361,271 | `6e742170d309016e5891a994e1ce1559c702a2ccd0075e67ef7157974f6406cb` |
| `voices-v1.0.bin` | 28,214,398 | `bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d` |

Each URL is
`https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/<filename>`.
The byte counts were checked against GitHub release metadata and reproduced,
with the digests, from downloaded assets using `wc -c` and `sha256sum`.
Muniment selects the INT8 graph for the CPU and memory baseline; FP16, FP32,
GPU, `main`, and the later `model-files-v1.1` release are outside this decision.

The fixed output is mono, 24,000 Hz, normalized `float32` PCM at speed `1.0`.
The native boundary validates that all samples are finite and in `[-1, 1]`;
playback converts from this one canonical format. The default voice is American
English `af_heart`. A voice name must exist in the verified bundle and must
match the selected language family; arbitrary voice files and blends are not
accepted.

### Language and text processing

The supported read-aloud claim for this slice is **English only**, with
American (`en-us`) and British (`en-gb`) pronunciation. The bundle also
contains voices labelled for Spanish, French, Hindi, Italian, Brazilian
Portuguese, Japanese, and Mandarin, but upstream warns that non-English G2P
can be weak or thin. Their presence is not a product support claim; adding a
language requires a pinned G2P pipeline and target-language ear tests.

English G2P uses an unmodified **eSpeak NG 1.52.0** executable, tag commit
`4870adfa25b1a32b4361592f1be8a40337c58d6c`, invoked as a bounded local child
process with `en-us` or `en-gb`, IPA, punctuation, and stress enabled. It is
not dynamically or statically linked into Muniment. The executable and its
data are app-owned; PATH, a system eSpeak installation, environment overrides,
and downloads at run time are forbidden. The native normalizer and
post-phonemization substitutions reproduce `kokoro-onnx` commit
`6843c53fc280ab130b7a8d206ebd3407e094efdc`, including whitespace/quote,
number, time, currency, abbreviation, punctuation, and Kokoro-vocabulary
filtering behavior. Conformance fixtures cover both accents and malformed,
empty, numeric, currency, URL, code, emoji, and mixed-script input.

Input is valid Unicode text. Normalize to NFC, replace invalid scalar/control
input (except line breaks and tabs) with spaces, then apply the pinned
normalizer. Empty or phoneme-empty input returns a typed no-content result and
does not start inference. Read-aloud does not interpret Markdown: the later
integration slice must first derive visible response text without hidden
markup, URLs, or code metadata and pass only that text to this boundary.

Segmentation is deterministic and occurs after normalization and English G2P.
Sentence punctuation is preferred, then clauses, then whitespace. Adjacent
units are greedily packed toward **100–200 Kokoro vocabulary tokens**. A
segment may contain at most **400 tokens**. A segment below **20 tokens** is
merged with an adjacent segment when the combined result remains at most 400;
otherwise it is synthesized as-is so short replies are never dropped or
padded with invented speech. An individual overlong unit is split at the last
whitespace before 400 tokens, or at the token boundary only when no whitespace
exists. Boundaries preserve every normalized character exactly once and never
split a Unicode scalar, number, URL, or abbreviation when a preceding safe
boundary exists. Tests must assert no empty, duplicate, omitted, or over-400
segment at 0, 1, 19, 20, 200, 400, and 401-token boundaries.

### Cancellation and privacy boundary

Each request has a monotonically increasing generation id and cancellation
token. Cancellation sets ONNX Runtime's terminate flag for an active run,
kills a still-running G2P child, stops scheduling segments, clears unplayed PCM,
and returns `cancelled` rather than partial success. Already handed-off audio
is stopped and drained by the later playback slice. Results from an older
generation are discarded even if they race with cancellation or a replacement
request. Shutdown uses the same path and waits for the worker; no detached
inference or G2P process may remain.

No input text, phonemes, voice vectors, generated PCM, or derived voice data
may leave the device or enter telemetry, crash reports, the control plane, Pi,
or llama-server. The inference and G2P boundaries expose no socket or HTTP
path. Artifact acquisition is a separate, explicit future operation; once the
artifacts are installed, synthesis performs no network access. Only playback
to the selected local audio device crosses the native synthesis boundary.

### Packaging, verification, and licenses

The supported targets match the desktop release baseline: macOS universal2
(`arm64` and `x86_64`), Windows `x86_64`, and Linux `x86_64`. CI must fail
closed for any other target. The installed runtime set contains target-native
ONNX Runtime v1.20.1 CPU libraries, the separately executable eSpeak NG 1.52.0
and its data, and the two model assets. No CUDA, CoreML, DirectML, system
library, Python, or package-manager fallback is allowed. Acquisition and the
choice to bundle or download these payloads remain a follow-up; either path
must use the same descriptors.

Before first use, and again after app update or any failed load, an app-owned
manifest verifies every expected filename, regular-file type, byte count, and
SHA-256 before publishing the complete set atomically. Directories and files
must not be links, and no extra voice/model file is selectable. A session opens
one verified revision snapshot; replacement cannot alter an active session.
Missing, partial, stale, or mismatched sets fail closed with repair available
and do not fall back to a mutable download or an OS voice under a Kokoro label.

ONNX Runtime v1.20.1 and the `kokoro-onnx` reference wrapper are MIT-licensed;
distributions must retain their copyright and permission notices. Kokoro v1.0
weights and the voice-style bundle are Apache-2.0; distributions must include
the license, preserve supplied notices, identify Hexgrad/Kokoro-82M and the
pinned converted assets, and mark Muniment modifications without implying
endorsement. The voices are model data under that same declared model license,
not MIT wrapper code.

eSpeak NG 1.52.0 is GPL-3.0-or-later. It remains a separate unmodified program
rather than a linked library. A distribution carrying it must provide the
complete corresponding source by a GPL-compliant method for the required
period, include GPLv3 and upstream notices, permit replacement/modification of
the executable, and provide installation information if legally required.
Release legal review must confirm this process boundary and source-delivery
plan before shipping; failure reopens the G2P choice rather than silently
using a system binary. `THIRD_PARTY_NOTICES.md` records the notices knowable at
decision time, and the packaged license inventory must be reconciled against
the actual native payload before release.

### Target-hardware validation matrix

Implementation does not inherit a performance or quality claim from upstream.
Run one signed/release-equivalent CPU build and the same versioned English
fixture corpus on these machines, or documented CPU-equivalent replacements:

| Tier | Required target |
| --- | --- |
| Low Windows | 4-core Intel Core i5-8250U, 8 GB RAM |
| Low Linux | 4-core Intel Core i5-8250U, 8 GB RAM |
| Mid Windows/Linux | 6-core AMD Ryzen 5 5500U, 16 GB RAM |
| Low macOS | Apple M1, 8 GB RAM |
| Mid macOS | current base Apple Silicon, 16 GB RAM |
| Intel macOS | 4-core Intel Core i5, 8 GB RAM |

For cold and warm runs, synthesize fixed 20-, 100-, 200-, and 400-token texts,
a 10-minute multi-segment response, and cancellation during G2P, first
inference, and between segments. Record model-load time, time to first playable
PCM, total synthesis wall time, audio duration, real-time factor (`wall time /
audio duration`), and process peak RSS. The gate on every target is warm RTF
**≤ 1.0**, p95 time to first playable PCM **≤ 1.0 s**, cold load **≤ 5.0 s**,
and peak RSS **≤ 1.0 GiB**, with no crash, leak, or unbounded growth over 100
requests. Cancellation must silence and drain within **200 ms**, leave no child
process, and permit the next request to speak correctly; no post-cancel buffer
from a stale generation may play.

Two listeners ear-test `af_heart`/`en-us` and at least one British voice with
`en-gb` across prose, headings, punctuation, questions, names, acronyms,
numbers, dates, money, URLs, code-adjacent text, short replies, and 400-token
segments. Both must find speech intelligible, accent/voice selection correct,
no missing or repeated text, no boundary clicks, truncation, long-segment
rushing, or unacceptable short-input degradation. Any failed numeric,
cancellation, memory, or ear-test gate reopens runtime, artifact, or
segmentation selection; it is not waived per platform.

The validation runner pins its fixture manifest, expected normalized text and
phoneme output, artifact descriptors, app/runtime revisions, machine/OS/CPU,
and raw per-run metrics in a reviewable report. This ADR leaves that runner,
physical runs, acquisition, playback integration, response context menu,
titlebar stop control, and settings UI to later slices.

## Considered alternatives

- Ship the Python `kokoro-onnx` package. Rejected: Python and its broad package
  graph are not needed for a two-file native inference boundary.
- Use FP32 or FP16 graphs. Rejected for the baseline package size and CPU/memory
  target; quality remains protected by ear tests.
- Advertise every language represented in the voice bundle. Rejected until
  each has a reproducibly pinned G2P path and target-language validation.
- Use system eSpeak NG. Rejected because its version, data, availability, and
  phonemes vary by machine. Linking bundled eSpeak into the closed process is
  also rejected; the separate executable boundary keeps its GPL obligations
  explicit.
- Use an OS voice fallback. It may remain a separately labelled product option,
  but it cannot satisfy or mask the reproducible Kokoro contract.

## Consequences

The native slice has one small CPU graph, one complete voice bundle, a stable
PCM contract, deterministic bounded work, race-safe cancellation, and an
honest English-only claim. The payload is about 120.6 MB before native runtime
and G2P files. INT8 quality and the eSpeak distribution plan remain explicit
release gates rather than assumptions.

## Sources

- Pinned model release: <https://github.com/thewh1teagle/kokoro-onnx/releases/tag/model-files-v1.0>
- Pinned wrapper source: <https://github.com/thewh1teagle/kokoro-onnx/tree/6843c53fc280ab130b7a8d206ebd3407e094efdc>
- Kokoro model and voice documentation: <https://huggingface.co/hexgrad/Kokoro-82M/blob/main/VOICES.md>
- ONNX Runtime v1.20.1: <https://github.com/microsoft/onnxruntime/releases/tag/v1.20.1>
- eSpeak NG 1.52.0: <https://github.com/espeak-ng/espeak-ng/releases/tag/1.52.0>
