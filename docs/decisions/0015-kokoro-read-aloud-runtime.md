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
`5c1b7ccbff7e5141c1da7a9d963d660e5741c319`. The full Python `kokoro-onnx`
package and its inference path are not shipped; its tokenizer module is vendored
unchanged for the G2P worker. Its MIT-licensed implementation at model-release commit
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

English G2P uses the reference stack itself: **CPython 3.12.8** (tag commit
`2dc476bcb9142cd25d7e1d52392b73a3dcdf1756`),
**phonemizer-fork 3.3.1**, and **espeakng-loader 0.2.4**, including its bundled
**eSpeak NG 1.52.0 shared library and data**. Package versions, transitive
versions, source/wheel filenames, byte sizes, and SHA-256 hashes are fixed by
`uv.lock` at `kokoro-onnx` commit
`6843c53fc280ab130b7a8d206ebd3407e094efdc`; installation must use that lock
with hash verification and no dependency re-resolution. The app invokes the
pinned `Tokenizer.normalize_text` and `Tokenizer.phonemize` implementation in
an app-owned, isolated Python worker process. Whole-input normalization occurs
exactly once; each source slice is then phonemized with `norm=False`. That call
fixes `preserve_punctuation=True`
and `with_stress=True`, uses the bundled shared library/data paths, and applies
the revision's ordered normalization, `kokoro` pronunciation corrections,
`ʲ→j`, `r→ɹ`, `x→k`, `ɬ→l`, hundred/z/ninety regex corrections, vocabulary
filter, and outer trim. PATH, `PHONEMIZER_ESPEAK_LIBRARY`, locale/environment
overrides, system eSpeak, package-manager fallback, and run-time downloads are
forbidden. Conformance means exact UTF-8 phoneme strings and token-id vectors
from that function and revision for both accents; fixtures cover malformed,
empty, numeric, currency, URL, code, emoji, and mixed-script input.

Input is valid Unicode text. Normalize to NFC, replace invalid scalar/control
input (except line breaks and tabs) with spaces, then apply the pinned
normalizer. Empty or phoneme-empty input returns a typed no-content result and
does not start inference. Read-aloud does not interpret Markdown: the later
integration slice must first derive visible response text without hidden
markup, URLs, or code metadata and pass only that text to this boundary.

Segmentation partitions normalized **source text before G2P**. Source slices
are contiguous half-open UTF-8 ranges whose concatenation is exactly the
normalized text; punctuation belongs to the slice on its left and intervening
whitespace belongs to the slice on its right. Empty edge whitespace is removed
only by the pinned normalizer. Before choosing boundaries, scan left-to-right
and mark URL-like non-whitespace runs beginning `http://`, `https://`, or
`www.`, numeric runs matching `[+\-]?[0-9][0-9,.:/\-]*%?`, and dotted initialism
or title runs matching `(?:[A-Za-z]\.){2,}|(?:Dr|Mr|Ms|Mrs)\.` as protected.
Matches use leftmost-longest order as listed and cannot overlap. Offsets below
are UTF-8 byte offsets at Unicode-scalar boundaries. A whitespace run is never
split: its entire byte range belongs to the slice on its right, so a boundary
adjacent to whitespace is at the run's start (after any punctuation or closing
quotes/brackets on its left), never inside or after the run.

For each current start offset, natural candidate ends have this strict rank:

1. after `.?!` and any immediately following closing quotes or brackets, but
   before a following whitespace run;
2. after `,;:` and any immediately following closing quotes or brackets, but
   before a following whitespace run;
3. at the start of a whitespace run (a word boundary), provided the preceding
   scalar is not punctuation or a closer already covered by rank 1 or 2.

Candidates inside a protected run are forbidden. Within the first non-empty
rank, choose the farthest candidate whose exact source slice is at most the
applicable token limit; lower ranks are considered only when the higher rank
has no candidate within that limit. A candidate's token count is obtained by
phonemizing that exact source slice with the selected accent and counting the
result after Kokoro vocabulary filtering; source character count is never used
as a proxy.

Starting at offset zero, apply that ranked search with a **200-token** limit.
If no natural candidate advances, repeat it with a **400-token** limit. If no
natural candidate advances at 400, use the farthest otherwise-allowed
Unicode-scalar end at or below 200 as an explicit hard-split fallback. This is
the only rule that may split an ordinary word. Boundaries inside whitespace
runs remain forbidden. If the slice starts at a protected run (ignoring
right-owned leading whitespace) whose complete end is over 400, instead waive
protection for that run only and choose its farthest scalar end at or below
400. Thus a protected run is split only when the run itself cannot fit within
400. If neither fallback advances, or even one scalar produces more than 400
tokens, return a typed
`unsupported-input` error rather than truncate it. An end-of-input offset is a
rank-1 natural candidate, ensuring the final remainder is consumed. Repeat to
end of input.
Then make one left-to-right repair pass: for each segment below **20 tokens**,
merge it with its right neighbor if re-phonemizing the combined source is at
most 400; for the final segment (or when the right merge exceeds 400), merge
left if that recomputed result is at most 400; otherwise retain it. After a
right merge, replace the pair and continue with the next segment; after a left
merge, replace the pair and continue after it. Never make a second pass.
Finally phonemize each resulting source slice once for model
input. No slice may be empty or exceed 400 tokens; short replies are retained,
and no source byte is duplicated or omitted.

Normative boundary examples use `K(s)` for the pinned phonemize-and-filter
token count. Repeated letters below denote fixture strings having the stated
UTF-8 byte length and token count; fixtures also record exact phonemes and ids.
The ranges shown are the required initial packing ranges, before the separately
specified short-segment repair:

- For `A + ".  \n" + B + "!"`, let `A` be 100 ASCII bytes with `K(A+".")=190`,
  `B` be 100 ASCII bytes with `K("  \n"+B+"!")=190`, and the combined count be
  over 200. The source is 205 bytes. Required ranges are `[0,101)` and
  `[101,205)`: the rank-1 end follows the period, and bytes 101--103 (two
  spaces and newline) stay together at the start of the right slice.
- For the 250-byte ordinary word `W` with `K(W)=250` and one token per scalar,
  there is no natural candidate before end-of-input and the end is over 200
  but within 400. The 400 search therefore selects `[0,250)`; it must not split
  at byte 200.
- For `W + " " + X`, where `W` is a 450-byte ordinary word with one token per
  scalar, `X` is 151 ASCII bytes, `K(" "+X)=152`, and
  `K(W[200:]+" "+X)=402`, no natural candidate fits the first 400 tokens. The
  explicit 200 hard fallback requires initial ranges `[0,200)`, `[200,450)`,
  and `[450,602)`; the second range ends at the word boundary and the single
  space at byte 450 belongs to the rightmost range.
- For protected URL `U`, 450 ASCII bytes with one token per scalar, the
  protected run has no complete end at or below 400, so only it loses
  protection. The 400 fallback requires ranges `[0,400)` and `[400,450)`; no
  byte is discarded or duplicated.
- For `2026-07-22 update`, byte range `[0,10)` is protected even when
  normalization changes its token count. If `K("2026-07-22")<=400`, no boundary
  may occur at bytes 1--9; packing uses `K`, not source character count.

Tests must assert the exact source ranges, phonemes, and token ids, with no
empty, duplicate, omitted, or over-400 segment at 0, 1, 19, 20, 200, 400, and
401-token boundaries and for each example above.

### Cancellation and privacy boundary

Each request has a monotonically increasing generation id and cancellation
token. Cancellation sets ONNX Runtime's terminate flag for an active run,
interrupts and joins the G2P worker, stops scheduling segments, clears
unplayed PCM,
and returns `cancelled` rather than partial success. Already handed-off audio
is stopped and drained by the later playback slice. Results from an older
generation are discarded even if they race with cancellation or a replacement
request. Shutdown uses the same path and waits for the worker; no detached
inference or G2P worker may remain.

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
ONNX Runtime v1.20.1 CPU libraries, CPython 3.12.8 and the fully locked G2P
environment (including phonemizer-fork and the eSpeak NG shared library/data),
and the two model assets. No CUDA, CoreML, DirectML, system library, or
package-manager fallback is allowed. Acquisition and the choice to bundle or
download these payloads remain a follow-up; either path must use the same
descriptors.

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

phonemizer-fork 3.3.1 is GPL-3.0 and eSpeak NG 1.52.0 is GPL-3.0-or-later;
CPython 3.12.8 is PSF-2.0. Distributions must include their licenses and notices,
provide complete corresponding source for the GPL components by a compliant
method for the required period, and satisfy replacement/installation-information
requirements where applicable. espeakng-loader 0.2.4 does not declare a
license in its wheel metadata, so legal approval of its redistribution and a
reconciled notice inventory for every locked transitive package are release
gates. Failure reopens the G2P choice. `THIRD_PARTY_NOTICES.md` records the
notices knowable at decision time.

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
requests. Cancellation must silence and drain within **200 ms**, leave no G2P
worker, and permit the next request to speak correctly; no post-cancel buffer
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

- Ship the Python `kokoro-onnx` inference package. Rejected: inference remains
  native ONNX Runtime. Its locked Python G2P stack is retained because it is
  the selected reference behavior and avoids inventing an incompatible CLI.
- Use FP32 or FP16 graphs. Rejected for the baseline package size and CPU/memory
  target; quality remains protected by ear tests.
- Advertise every language represented in the voice bundle. Rejected until
  each has a reproducibly pinned G2P path and target-language validation.
- Use system or CLI eSpeak NG. Rejected because version, data, invocation, and
  wrapper postprocessing can vary and do not reproduce the pinned reference.
- Use an OS voice fallback. It may remain a separately labelled product option,
  but it cannot satisfy or mask the reproducible Kokoro contract.

## Consequences

The native slice has one small CPU graph, one complete voice bundle, a stable
PCM contract, deterministic bounded work, race-safe cancellation, and an
honest English-only claim. The payload is about 120.6 MB before native runtime
and G2P files. INT8 quality and the G2P distribution plan remain explicit
release gates rather than assumptions.

## Sources

- Pinned model release: <https://github.com/thewh1teagle/kokoro-onnx/releases/tag/model-files-v1.0>
- Pinned wrapper source: <https://github.com/thewh1teagle/kokoro-onnx/tree/6843c53fc280ab130b7a8d206ebd3407e094efdc>
- Kokoro model and voice documentation: <https://huggingface.co/hexgrad/Kokoro-82M/blob/main/VOICES.md>
- ONNX Runtime v1.20.1: <https://github.com/microsoft/onnxruntime/releases/tag/v1.20.1>
- eSpeak NG 1.52.0: <https://github.com/espeak-ng/espeak-ng/releases/tag/1.52.0>
