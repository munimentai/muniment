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

## Removal and collection

`remove` deletes one object and succeeds when that object is already absent.
`object_hashes` lazily enumerates only regular files at canonical object paths;
unrelated files and directories under `objects/` are ignored. `collect_unreferenced`
accepts a keep-set (normally `RunJournal::referenced_hashes()`), removes every
enumerated object outside that set, and returns the hashes it removed. It does
not inspect or remove root entries, including `.cas-tmp-*` files; the temp-file
sweep above exclusively owns those files.

Collection must be serialized with CAS puts and journal appends. The caller must
hold the application's shared state lock from taking the journal reference
snapshot through completion of the CAS sweep; otherwise an object published or
referenced after the snapshot could be removed while in use.

## Chat attachments

`attachment::ingest_attachment` is the pure-core boundary for local chat files.
It accepts a reader and untrusted display metadata, strips path components from
the display name, validates the optional media type and declared byte length,
and streams the bytes through `LocalCas::put_reader`. Its version 1
`chat.attachment.ingested` journal event records only the validated content
hash, safe display name, byte length, and optional media type; the source path is
neither retained nor serialized. Attachment payloads participate in the same
`RunJournal::referenced_hashes()` accounting and durable export as other CAS
payloads.

The caller must hold the application's shared state lock across the entire
ingestion call. CAS publication always completes before the journal append, so a
successful journal reference can never point at an object that this operation
has not published. A byte-count mismatch or journal-append failure can leave a
published but unreferenced object. `AttachmentIngestError` reports these cases
explicitly (including the published hash or descriptor); the object is safe for
the existing unreferenced-object collector to remove.

## Desktop adapter

The desktop app opens one `LocalCas` at `<app-data>/cas` beside its
`<app-data>/runs.sqlite3` journal. Both are owned by the same locked storage
state. For a new chat, the adapter appends and applies `run.started`, then
streams each explicitly selected regular file into CAS and appends its
`chat.attachment.ingested` event using the same run sequence and projector.
Only after every object verifies and every event is durable may the coordinator
submit the text prompt to Pi. File opening, hashing, verification, and journal
I/O run on a blocking worker rather than the async UI thread.

This boundary is local durability only. An ingested attachment has not been
uploaded, processed by a cloud service, or supplied to Pi. Source filesystem
paths are transient adapter inputs and never appear in journal payloads,
history, command results, or user-facing errors.
