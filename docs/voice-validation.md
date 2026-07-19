# Parakeet target-hardware validation

The validation runner decodes a versioned, local WAV corpus with the production
offline recognizer and writes a comparable JSON report. It performs no network
requests and never downloads a model or audio. Build and run the same release
binary on every ADR 0004 target:

```sh
cargo run --release --manifest-path src-tauri/Cargo.toml -p muniment-core \
  --bin parakeet-eval -- \
  --model-root /path/to/asr-lifecycle-root \
  --manifest /path/to/corpus/manifest.json \
  --output /path/to/results/low-linux.json \
  --machine-tier low-linux-i5-8250u-8gb
```

`--model-root` is the explicit model lifecycle root containing the verified
`current` pointer and `revisions/` directory defined by ADR 0005. Construction
uses `OfflineParakeetRecognizer::from_verified_current`, so missing, modified,
or unpinned model files fail before decoding. Corpus paths are relative to the
manifest, must already exist, and cannot be absolute.

## Corpus manifest version 1

Audio must be non-empty mono 16 kHz WAV containing 16-bit PCM or normalized
32-bit float samples. The declared duration must match the WAV duration to
within one sample (with a 1 ms minimum tolerance).
The manifest must include both a clean and noisy case at each ADR-required
duration (5, 15, and 60 seconds). A declared duration within 0.25 seconds of a
required duration fills that matrix slot; the stricter WAV-duration check still
ensures the declaration describes the actual audio.

```json
{
  "schema_version": 1,
  "cases": [
    {
      "id": "en-clean-5s-01",
      "language": "en",
      "audio_class": "clean",
      "duration_seconds": 5.0,
      "audio_path": "audio/en-clean-5s-01.wav",
      "reference_transcript": "Optional human transcript"
    },
    {
      "id": "de-noisy-15s-performance-only",
      "language": "de",
      "audio_class": "noisy",
      "duration_seconds": 15.0,
      "audio_path": "audio/de-noisy-15s.wav"
    }
  ]
}
```

The report records schema/model/platform/machine identity, per-case transcript,
decode wall time, RTF, and identical first/final transcript latency measured
from the first PCM sample (audio duration plus decode wall time, because the
model is offline). References produce deterministic case-insensitive
whitespace-word WER and insertion/deletion/substitution counts. Aggregates
include cold model load and nearest-rank p50/p95 values for first-PCM-to-first-
transcript latency, first-PCM-to-final-transcript latency, and post-end-of-
speech offline decode latency. Process peak RSS is JSON `null`, never zero, on
platforms where the runner cannot measure it.

## 100-utterance endurance gate

On each ADR 0004 target machine, run the same release binary and corpus with
the bounded endurance option:

```sh
cargo run --release --manifest-path src-tauri/Cargo.toml -p muniment-core \
  --bin parakeet-eval -- \
  --model-root /path/to/asr-lifecycle-root \
  --manifest /path/to/corpus/manifest.json \
  --output /path/to/results/low-linux-endurance.json \
  --machine-tier low-linux-i5-8250u-8gb \
  --endurance-utterances 100
```

This loads one verified recognizer, then performs exactly 100 consecutive
decodes by cycling through the manifest in order. The report's `decode_run`
object must show both `requested_decode_count` and `completed_decode_count` as
100. Its `start_process_resident_memory_bytes` and
`end_process_resident_memory_bytes` are current resident-memory observations
taken immediately before and after those decodes; compare them for ADR 0004's
no-unbounded-growth gate. The aggregate `process_peak_rss_bytes` is the process
lifetime high-water mark used for the separate 2 GiB peak-RSS gate. Unsupported
memory measurements are JSON `null`. The report retains at most one case object
per manifest case rather than one object per endurance iteration. A decode
failure exits in the existing redacted CLI error format and does not write a
partial report.

Omitting `--endurance-utterances` preserves the normal single-pass behavior:
each manifest case is decoded once. Values outside 1 through 100 are rejected.

Keep approved corpus audio and transcripts outside this repository. Preserve
the manifest and generated JSON reports together when comparing matrix runs.
