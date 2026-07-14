# ADR 0010: Native model-install filesystem adapters

## Status

Accepted

## Context

Model installation needs an exclusive lock shared by app processes and a way
to query free space on the volume containing the model store. The Rust standard
library version used by the project does not provide both operations across the
supported Linux, macOS, and Windows targets.

## Decision

Use `fs2` for advisory file locking and available-space queries. It is a small,
widely used crate that provides both required operations, avoiding separate
platform FFI implementations or multiple dependencies. Holding the locked file
open makes lock cleanup process-safe: the operating system releases the lock if
the process exits or the guard is dropped.

## Consequences

Native install adapters gain one dependency. If the project's minimum Rust
version reaches a release with stable cross-platform standard-library file
locking, the locking half can move to `std`; a platform free-space implementation
or dependency will still be required.
