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
include cold model load, nearest-rank p50/p95 decode wall latency, and process
peak RSS. Peak RSS is JSON `null`, never zero, on platforms where the runner
cannot measure it.

Keep approved corpus audio and transcripts outside this repository. Preserve
the manifest and generated JSON reports together when comparing matrix runs.
