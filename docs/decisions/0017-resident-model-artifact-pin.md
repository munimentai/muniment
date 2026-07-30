# 0017 — Pin the resident Qwen3.5-4B model artifact

- Status: superseded by the 2026-07-29 cloud ingress ruling
- Date: 2026-07-27
- Context: harness-spec §15.3; ADR 0003, 0006, 0014; owner decision 2026-07-27

## Context

[ADR 0003](0003-resident-gemma-model.md) pinned Google's Gemma 3 4B QAT Q4_0
GGUF as the single resident local model. Since MUNIDESK-391 (2026-07-20) the
code has verified and launched a different artifact: `RESIDENT_MODEL` in
`src-tauri/core/src/llama.rs` pins a Qwen3.5-4B Q4_K_M GGUF served under the
alias `muniment-required-qwen3.5-4b`.

No decision record captured that change. ADR 0003 stayed `accepted`,
`docs/sidecar.md` kept naming the retired Gemma alias as the stable API alias,
and harness-spec's licence inventory kept listing the polish/classifier row as
Gemma under "Gemma Terms of Use" — the wrong licence for the artifact actually
shipped. The contradiction misled an operator for a full session on
2026-07-27. On a product whose pitch is verifiable provenance, the decision
record has to name the bytes that ship.

## Decision

This ADR supersedes ADR 0003 and is the pinned identity of muniment's single
resident local model. ADR 0003 is kept as the record of the earlier decision
and is marked superseded.

### Why the model changed

- **The resident role grew.** harness-spec §15.3 made one required on-device
  artifact serve onboarding/import triage and the per-query front-door router
  in addition to ADR 0003's dictation polish and routing classification. The
  triage role reads an approved export and emits a structured report, so the
  served context limit stopped being a detail: this artifact serves 262,144
  tokens against Gemma 3 4B's 131,072.
- **It is smaller.** 2.74 GB Q4_K_M against ADR 0003's 3.15 GB QAT Q4_0, on a
  required first-use download that every installation pays for.
- **The licence is plain.** Apache-2.0 removes the Gemma Terms of Use
  acceptance gate, the section 3.2 use restrictions muniment would have had to
  carry into its own terms as enforceable provisions, and the section 3.1
  notice wording from the acquisition path. The legal read of the Gemma Terms
  that harness-spec §10 carried as a pre-sale gate is no longer a release gate
  for the resident model.

### Pinned artifact

Every value below is byte-for-byte the corresponding field of the compiled
`RESIDENT_MODEL` descriptor in `src-tauri/core/src/llama.rs`. Numbers are
written with the descriptor's digit grouping.

| Descriptor field | Pinned value |
| --- | --- |
| `source_url` | `https://huggingface.co/munimentai/Qwen3.5-4B-GGUF/resolve/0ddb6039fd5a9d75a8a7fd03227de02c9e97daae/qwen3.5-4b-Q4_K_M.gguf` |
| `license` | `Apache-2.0` |
| `filename` | `qwen3.5-4b-Q4_K_M.gguf` |
| `byte_size` | `2_783_446_784` |
| `sha256` | `5ca0d868d45462e33c7671740bbd97b1ec4d38834827609fcd1a3f726cf49649` |
| `alias` | `muniment-required-qwen3.5-4b` |
| `context_tokens` | `262_144` |

The immutable repository revision `0ddb6039fd5a9d75a8a7fd03227de02c9e97daae` is
also compiled as `RESIDENT_MODEL_REVISION` and is the only revision the
downloader may form a URL from. The revision identity published on disk is
`qwen3.5-4b-instruct-q4_k_m-v1`, beneath the app-data root
`models/qwen3.5-4b/`.

ADR 0003's verification and identity rules stand unchanged: at every local
launch the core requires a regular file of the pinned byte size, streams it
through SHA-256, and fails closed before spawning `llama-server`; the alias is
passed separately with `--alias` and used in chat requests, so neither API
clients nor model identity depend on an installation path. Changing model,
quant, revision, or served context remains an explicit descriptor change and a
new ADR — not an amendment to this table alone.

### The record cannot drift again

`src-tauri/core/tests/resident_model_adr.rs` parses the table above and asserts
each pinned value against the matching `RESIDENT_MODEL` field. Editing the
descriptor without editing this ADR, or the reverse, fails
`cargo test --manifest-path src-tauri/core/Cargo.toml`; so does dropping,
duplicating, reordering, or unquoting a row, or renaming this file out of the
`0017-` number. Because a markdown-only pull request otherwise skips the Rust
suite, CI classifies a change to `docs/decisions/0017-*.md` as one that must run
it. `test/smoke.sh`, which runs on every pull request, additionally holds this
record's accepted status, ADR 0003's superseded status, and the absence of the
retired Gemma alias or any Gemma-as-resident-model wording in `docs/`.

This guard is why the table above is the only place in `docs/` that restates
the pinned values: every other document points here instead of copying them.

### Artifact custody

Owner decision 2026-07-27: muniment builds and owns its model artifacts and
hosts them in the muniment Hugging Face organization. The resident pin names
the muniment-built GGUF. Its upstream source revision is
`851bf6e806efd8d0a36b00ddf55e13ccb7b8cd0a`.

## Consequences

- The decision record, the compiled descriptor, the sidecar contract, and the
  licence inventory now name one artifact and one licence, and a test keeps
  them that way.
- Installations pay a 2.74 GB required first-use download instead of 3.15 GB,
  and the acquisition path carries an Apache-2.0 attribution notice rather
  than a Gemma terms-acceptance gate.
- [ADR 0006](0006-resident-gemma-model-lifecycle.md) continues to govern
  acquisition, staging, atomic publication, update/rollback, and removal for
  this artifact; its Gemma-specific terms and notice obligations do not apply
  to an Apache-2.0 artifact, as noted at the head of that record.
- The `Gemma*` type, command, and pointer names in the code and the historical
  Gemma wording inside earlier ADR bodies are now vocabulary debt. Renaming
  them is tracked separately; this ADR deliberately changes no code symbol.
- The resident model has now changed once ahead of its record. Any future
  descriptor change is expected to land with its ADR edit in the same commit,
  which the guard test enforces mechanically rather than by review discipline.

## Sources

- Compiled descriptor: `src-tauri/core/src/llama.rs` (`RESIDENT_MODEL`,
  `RESIDENT_MODEL_REVISION`)
- Superseded pin: [ADR 0003](0003-resident-gemma-model.md)
- Lifecycle and publication contract: [ADR 0006](0006-resident-gemma-model-lifecycle.md)
- Serving runtime: [ADR 0014](0014-llama-server-distribution.md)
- Required on-device onboard/router role: [harness-spec §15.3](../spec/harness-spec.md)
- Resident artifact: <https://huggingface.co/munimentai/Qwen3.5-4B-GGUF/tree/0ddb6039fd5a9d75a8a7fd03227de02c9e97daae>
- Apache License 2.0: <https://www.apache.org/licenses/LICENSE-2.0>
