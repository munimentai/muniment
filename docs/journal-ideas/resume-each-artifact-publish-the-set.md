# Resume each artifact, publish the set

A local speech feature needed two immutable assets totaling 120,575,669 bytes: a 92,361,271-byte model and a 28,214,398-byte voice bundle. We implemented acquisition so each file resumes independently, but neither file becomes usable merely because its download completed. The stage is accepted only when the directory contains exactly the two pinned regular files and both byte counts and SHA-256 digests match.

The useful separation is between resumability and visibility. Partial bytes remain available for retry after cancellation, short reads, transient server failures, or retry backoff. Completed bytes are renamed into their staged filenames only after individual verification. The acquisition returns success only after verifying the complete two-file set, including rejection of symlinks, directories, and unexpected entries.

Tests exercise fresh and ranged downloads, a server that ignores a range and returns a full response, malformed content ranges, oversized and short bodies, cancellation during streaming and backoff, checksum failures, and hostile stage entries. They also verify aggregate progress across both artifacts and one shared deadline across retries.

This demonstrates a practical pattern for multi-file model delivery: preserve retryable work at the file boundary, but make readiness a property of the exact manifest as a whole. It does not yet demonstrate durable installation, inference correctness, audio quality, or target-device performance; those require separate publication, runtime, and hardware-validation work.
