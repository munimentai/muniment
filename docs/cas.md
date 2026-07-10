# Content-addressed storage

`muniment_core::cas::LocalCas` stores arbitrary local blobs by the lowercase
hexadecimal SHA-256 digest of their contents. It has no Tauri or network
dependency.

## On-disk layout

Objects live at `objects/<first two hash characters>/<remaining hash>`. Writes
are streamed through a fixed-size buffer into `.cas-tmp-*` files in the store
root while their digest is computed. Callers can use `put_reader` and
`open_object` to ingest and read large objects without buffering the complete
contents; `put` and `get` remain convenient APIs for small in-memory objects.

## Atomicity and deduplication

A completed, synced temp file is atomically linked to its final object path, so
readers never observe a partial object. Publication does not replace an object
already at that path: concurrent or repeated writes of the same content discard
their temp file and return the same hash. Because temp files and objects share a
filesystem, this also avoids a copy during publication.

## Temp-file sweep

Opening a store removes regular files in the store root whose names begin with
`.cas-tmp-` and whose modification times are at least 24 hours old. The age
threshold protects a live writer in another process; consequently, crash debris
may remain for up to one day. Fresh temp files, other root entries, and everything
under `objects/` are left untouched.

## Verification

`verify` streams an object's bytes through SHA-256 and compares the result with
its requested hash. It distinguishes missing objects from corrupt ones without
loading the object into memory.
