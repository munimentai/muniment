# 0028 — Bundle the on-device router classifier

- Status: accepted
- Date: 2026-09-04
- Context: owner ruling 2026-09-04, ADR 0027, harness-spec §15.3

## Context

The owner ruled that the desktop ships two local models. The router classifier
ships with the install. ADR 0027 separates it from the downloaded extractor.

The desktop already ships ONNX Runtime on every supported platform. The Linux
bundle includes `libonnxruntime.so`. The macOS bundle includes
`libonnxruntime.1.24.4.dylib`. The Windows bundle includes `onnxruntime.dll`.
The three `tauri.*.conf.json` files publish these libraries from
`src-tauri/third-party/sherpa-onnx-v1.13.2/`.

The `sherpa-onnx` bindings in `src-tauri/core/Cargo.toml` expose speech graphs
alone. They cannot create the text encoder session required by this classifier.

The operator has not supplied the classifier artifact, its SHA-256 digest, or
its byte size. The owner has not named the action that each class selects.

## Decision

The desktop bundle contains one classifier ONNX graph. The graph contains the
text encoder and its classifier head. Its descriptor pins the artifact name,
version, file name, byte size, SHA-256 digest, and license.

The classifier head and its ordered class list form one versioned contract.
The artifact digest pins the head together with its class list metadata. The
class list is exactly:

1. `route.cloud`
2. `route.local`
3. `route.proxy`

The ONNX graph records that ordered class list in its model metadata. The
loader compares the metadata with the taxonomy above before it creates a usable
classifier. A missing, reordered, or different class list fails closed. The
desktop does not classify with that graph.

A classifier-owned native adapter loads the platform's bundled ONNX Runtime
library from its exact application resource path. It resolves the ONNX Runtime
C API through `OrtGetApiBase`. The adapter creates the text encoder session
directly through that API. It does not route the graph through the speech-only
`sherpa-onnx` bindings. It adds no second ONNX Runtime library to the bundle.

This artifact has no download, no lifecycle pointer, and no user opt-in. The
installed application bundle is its only delivery path. An application update
replaces the pinned graph and its descriptor together.

No classifier result crosses the cloud-bound wire. Cloud requests retain only
the server-supplied grant values defined by the current request contract. The
cloud still classifies every request at ingress.

## Amendment — 2026-09-04

The graph stores the ordered class list under the `muniment.router.classes`
metadata key. The value uses a JSON array of strings. The loader reads and
validates this key before any session use.

## Open owner inputs

The operator must supply the artifact with its SHA-256 digest and byte size.
No bundling slice is fileable until those values exist.

The owner must name the action that each class selects. No consumer slice is
fileable until the owner answers this input.

## Consequences

- The desktop can classify locally without changing the cloud request.
- A taxonomy mismatch makes local classification unavailable.
- The classifier shares the bundled ONNX Runtime but not the speech bindings.
- The classifier adds no model download, publication pointer, or consent flow.
- The artifact pin and class actions remain blocked on the recorded owner inputs.
