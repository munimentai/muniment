# Public evidence — docs sync

This directory is muniment-desktop's public-evidence documentation. Any PR that
changes what an outside user can see or do — public API endpoints or wire
behavior, install/distribution channels or flags, authentication flows, or
other externally observable behavior — MUST update this directory in the same
PR.

Evidence files are written to be lifted verbatim into the public docs site
(muniment.ai/docs):

- plain factual reference prose;
- copy-pasteable commands and config;
- no roadmap speculation;
- no internal codenames;
- no pricing or monetization content (owner-only).

The reviewer blocks a PR that changes a public surface without updating
evidence. Merged evidence changes are picked up automatically by the site lane
— do not file site tickets by hand.

This is a markdown-only PR: CI gates on the structure smoke job alone. Do not
add evidence files for past changes in this PR — this slice only establishes
the directory and convention.
