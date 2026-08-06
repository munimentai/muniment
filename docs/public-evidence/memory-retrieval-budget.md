# Memory retrieval budget

The memory-search tool defaults to at most five returned items and a 250 ms
timeout. Its default character budget equals the selected model capability
record's `minimum_cacheable_prefix_characters` value.

The runtime reads that value for every selected model. It never substitutes a
constant or carries another model's value across a model change. This rule also
applies when a customer selects a model under BYOK.

A request may lower the item count, character budget, or timeout. It may not
raise any value above the configured limit. Zero items or zero characters
returns no content, and a zero timeout expires immediately.
