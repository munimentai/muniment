# Upstream provenance

- Repository: `https://github.com/microsoft/playwright`
- Immutable commit: `2bcd8f21ad032744763f2f0c7ba6e7006e13fa11`
- Import date: 2026-07-17
- Imported source path: `packages/playwright-core/src/tools/mcp/cdpRelay.ts`, specifically the `ExtensionConnection` relay request/response/event core
- Local destination: `src/index.ts`

## Local modifications

The upstream unit was extracted from its WebSocket server so this baseline has no browser, socket, or network dependency. The concrete `ws` transport was replaced with the injected `RelayTransport` interface. The class was renamed `RelayConnection` and made public; protocol types and a public event subscription were added. Browser launching, HTTP/WebSocket server setup, endpoint routing, logging, and Playwright runtime integration were not imported.

The extracted unit was adapted to strict standalone TypeScript and native private fields. Message size is bounded by UTF-8 bytes; malformed UTF-8, malformed JSON, invalid/contradictory protocol shapes, and oversized input close the transport with one deterministic protocol error. Requests are registered before sending to avoid a synchronous-response race. Send failure and transport closure reject all pending requests with stable redacted errors. Closure removes transport and event listeners even when transport cleanup throws. Remote error text is not exposed. Constructor and method inputs receive boundary validation. Stale response IDs are ignored, and exceptions from consumer event callbacks are isolated from the protocol lifecycle.

Tests and package/build configuration are new Muniment files and are not imported upstream.

The imported source retains Microsoft's Apache-2.0 copyright and license header. See `LICENSE` and the repository-level `THIRD_PARTY_NOTICES.md`.
