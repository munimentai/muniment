# 0019 — Generated cloud contract source of truth

- Status: accepted
- Date: 2026-07-29
- Context: E0 published cloud contracts; ADR 0009; ADR 0011

## Context

Each client surface currently vendors and validates its own copy of a published
cloud contract. Mobile copied `artifact-content/1` for MUNIMOBILE-240 and
`transcript-transformation/1` for MUNIMOBILE-227. Those copies drifted from the
published fixtures. MUNIMOBILE-585 then waited nine nights for a cloud
dependency, and MUNIMOBILE-610 ended the chain by copying the shipped fixture.

Muniment has Rust consumers in the Tauri core and CLI. Mobile and the editor
extension are TypeScript consumers. A package for one language cannot give all
four surfaces checked types and codecs.

The local attach contract already has a related ownership model. Rust types,
codecs, and fixture export live in `src-tauri/attach`, while
`protocol-fixtures/muniment.attach/1/` holds its golden wire examples. The
editor extension tests its TypeScript decoder against those examples. ADR 0009
sets the golden-fixture compatibility rule, and ADR 0011 keeps the CLI and
editor extension as peers over that desktop-owned protocol.

## Decision

### Schema and generation

The `mikeydiamonds/muniment-cloud` repository owns the canonical schemas at
`contracts/e0/<contract>/<major>/`. A contract name and major version therefore
have one writable source. Client repositories may contain generated output or
golden examples, but they must not contain another editable schema.

TypeSpec is the source format. Its model and service syntax keep the HTTP and
JSON contract readable while its OpenAPI emitter produces a standard,
machine-checkable generation boundary.

The repository emits OpenAPI 3.0 from TypeSpec. OpenAPI Generator then uses its
`rust` generator for a versioned Rust crate and its `typescript-fetch`
generator for a versioned TypeScript package. The Rust generator gives the
Tauri core and CLI native types, serialization, and HTTP code. The TypeScript
generator gives mobile and the editor extension the same features without a
second handwritten decoder.

Rust crates use `muniment-e0-<contract>-v<major>`. TypeScript packages use
`@muniment/e0-<contract>-v<major>`. The artifact version matches the contract
version.

Generation runs only in the cloud contract publication lane. That lane checks
the emitted OpenAPI document and generated artifacts for a clean rebuild. A
client repository consumes a published immutable artifact and never runs a
generator during its normal build.

### Consumers

| Surface | Import |
| --- | --- |
| Tauri core in `muniment-desktop` | The generated Rust crate for each used E0 cloud contract |
| Muniment CLI | The same generated Rust crates, without importing `muniment-core` |
| `muniment-mobile` | The generated TypeScript package for each used E0 cloud contract |
| Editor extension | The same generated TypeScript packages, without importing the desktop frontend |

Each surface uses the generated request types, response types, codecs, and
client methods. It may wrap those imports behind its own application adapter.
It must not restate a wire type, field validator, or endpoint client.

### Version and compatibility policy

Each contract has an independent semantic version. Its major version also
appears in the schema path and published artifact name. A surface pins one
exact artifact version in its package manifest and lockfile. Rust surfaces pin
the crate, while TypeScript surfaces pin the package.

Adding an optional field or enum value is backward compatible and increments
the minor version. A documentation or generator-only correction increments the
patch version when it leaves the wire contract unchanged. Removing or renaming
a field, changing its meaning or type, adding a required field, or changing
endpoint behavior requires a new major version.

Consumers ignore unknown optional fields. They must handle unknown enum values
through an explicit generated unknown case or fail with a contract-version
error. They must not silently map an unknown value to an existing meaning.
Cloud producers support the current major and the previous major until every
surface has shipped the current major. Dropping the previous major requires
usage evidence that no supported surface still sends it.

A published change first updates TypeSpec and its compatibility fixtures in one
cloud review. The publication lane emits both language artifacts from the same
schema revision, assigns the same contract version, and publishes them
together. Automation then opens version-pin updates for the desktop, CLI,
mobile, and editor extension. Each update runs that surface's generated-client
tests against the published compatibility fixtures. The cloud may make a new
optional response available before all updates merge. It cannot require new
input until every supported surface ships the compatible version. It also
cannot remove old behavior before that point.

### Migration of vendored contracts

`artifact-content/1` moves first. The cloud owner transcribes the shipped v1
fixture into TypeSpec. Both generated clients must accept every v1 example
before publication. Mobile removes its handwritten model and validator after
it pins the TypeScript artifact.
The Tauri core, CLI, and editor extension pin that artifact when they first
consume artifact content.

`transcript-transformation/1` follows the same path. The cloud owner transcribes
the shipped v1 fixture, checks generated serialization against every v1
example, and publishes both artifacts. Mobile then replaces its copied model
and validator with the generated TypeScript import. Each other surface pins
the matching artifact before it consumes transcript transformations.

Migration preserves the published v1 wire shape. If TypeSpec cannot express a
shipped edge without changing bytes or JSON meaning, the v1 schema records
that shape explicitly. A cleaner model requires v2. A surface removes its
handwritten copy only after its compatibility tests pass, so a partial
publication cannot leave it without a working contract.

This decision extends ADR 0009's cross-platform golden-fixture rule to
published cloud contracts. It does not replace the local attach contract's
Rust ownership or `protocol-fixtures/muniment.attach/1/`. The attach protocol
is desktop-owned local IPC, not an E0 cloud contract.

ADR 0011 also stands. The CLI and editor extension remain peer clients of the
desktop over `muniment.attach/1`. Generated cloud imports do not let either
surface open the journal, own credentials, spawn a runtime, or bypass attach
authorization.

### Alternatives considered

**Protobuf with `prost` and `protobuf-es`.** It gives strong native generation
and mature compatibility rules. Its wire model would constrain the published
HTTP and JSON contracts during a no-behavior-change migration.

**Plain JSON Schema 2020-12 with per-language generators.** It keeps payload
schemas readable, but its generators provide weaker endpoint clients and leave
HTTP operations split into another source.

**Handwritten OpenAPI.** It supports the selected client generators, but a
large YAML document makes model reuse and review harder than TypeSpec without
adding a compatibility benefit.

**One handwritten TypeScript package.** It cannot serve the Rust surfaces and
would preserve duplicate Rust validators.

**Continue golden fixtures without generated clients.** Fixtures catch known
wire drift but do not stop a surface from restating incomplete types or
validators.

## Consequences

- Every published E0 cloud contract has one editable schema and two generated
  language artifacts.
- Contract publication must publish both artifacts together and keep fixtures
  compatible with the declared version.
- Every surface gains compile-time types and one generated HTTP and JSON
  implementation, but it depends on a contract publication lane.
- Exact pins make updates deliberate and allow surfaces to release on their
  existing schedules.
- The existing attach protocol remains governed by ADR 0009 and ADR 0011.
- This ADR adds no generator, dependency, configuration, manifest change, or
  generated code.
