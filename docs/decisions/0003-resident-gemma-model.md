# 0003 — Pin the resident Gemma model artifact

- Status: accepted
- Date: 2026-07-10
- Context: ROADMAP Phase 2 item 10; harness-spec §8.2

## Decision

Muniment's single resident local model is Google's instruction-tuned **Gemma 3
4B QAT Q4_0 GGUF**, identified exactly as follows:

| Field | Pinned value |
| --- | --- |
| Upstream | `google/gemma-3-4b-it-qat-q4_0-gguf` on Hugging Face |
| Revision | `15f73f5eee9c28f53afefef5723e29680c2fc78a` |
| Filename | `gemma-3-4b-it-q4_0.gguf` |
| Byte size | `3,155,051,328` |
| SHA-256 | `76aed0a8285b83102f18b5d60e53c70d09eb4e9917a20ce8956bd546452b56e2` |
| Terms | <https://ai.google.dev/gemma/terms> |
| Application alias | `muniment-resident-gemma` |
| Context limit | 131,072 tokens |

The source revision, file size, and LFS digest are recorded by the upstream
repository metadata. The 4B instruction-tuned model is the smallest Gemma 3
variant in the roadmap's 3–4 GB artifact class with the 128K context documented
for Gemma 3 4B. Google's QAT Q4_0 release avoids a locally converted,
under-specified quant and is intended for efficient deployment. One loaded
model will eventually serve both dictation polish and classification without a
reload.

At every local launch, the core requires a regular file of the pinned size and
streams it through SHA-256 verification before spawning llama-server. The core
passes the stable alias separately with `--alias` and uses it in chat requests;
neither API clients nor model identity depend on an installation path.

## Consequences

Startup performs one sequential read of the artifact and fails closed if it is
missing, unreadable, the wrong size, or has a different digest. This costs
startup I/O but makes the local execution identity reproducible and prevents a
partial or substituted artifact from being served. Changing model, quant,
revision, or runtime context is an explicit descriptor and ADR update.

Artifact acquisition, update/rollback policy, packaging, and acceptance of the
Gemma terms are separate distribution concerns. Dictation and classifier role
prompts and their evaluation are also deferred.

## Sources

- Upstream artifact: <https://huggingface.co/google/gemma-3-4b-it-qat-q4_0-gguf/tree/15f73f5eee9c28f53afefef5723e29680c2fc78a>
- Gemma 3 model card: <https://ai.google.dev/gemma/docs/core/model_card_3>
- llama-server model alias behavior: <https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md>

