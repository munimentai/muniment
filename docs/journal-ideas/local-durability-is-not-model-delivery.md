# Local durability is not model delivery

We added durable local file attachments to a desktop chat and found an important boundary in the product language: saving a file safely is not the same as sending it to a model.

The implementation stores attachment bytes in a content-addressed local store and journals only the durable attachment record. The chat-facing projection deliberately exposes just the display name, byte length, and optional media type; it does not expose the source path or content hash. Both the live conversation and restored history render the same explicit status: `Saved locally · not sent to model`.

Contract tests cover ingestion, projection, live results, and restored history. That demonstrates consistent behavior across the local durability and replay paths. It does not demonstrate upload, provider delivery, or model consumption; those paths were intentionally not implemented.

The broader lesson is that attachment state should describe completed system boundaries, not user intent. A paperclip chip can easily imply that a model received a file even when only local persistence succeeded. Naming the exact completed transition makes partial implementations honest, makes recovery comprehensible, and leaves later delivery work with a clear state change to implement and test.
