# 0001 — Frontend framework: Svelte 5 (+ Vite, tokens as CSS custom properties)

- Status: accepted
- Date: 2026-07-09
- Context: ROADMAP Phase 2 item 8 (first slice); harness-spec §6 ("Frontend
  framework: team's choice (React or Svelte), Tailwind fine")

## Decision

**Svelte 5** as the frontend framework, built with **Vite**, with the
design-token layer implemented as **plain CSS custom properties** (no Tailwind
for now). The frontend compile is driven entirely by
`build.beforeDevCommand` / `build.beforeBuildCommand` in `src-tauri/tauri.conf.json`,
so `npm ci && npm run tauri build` remains the only thing CI needs to run.

## Rationale

**Bundle size and runtime weight.** Svelte compiles components to imperative
DOM code with a small shared runtime (~10–15 KB min+gz for an app this size)
versus React's ~45 KB baseline (react + react-dom) plus per-component VDOM
overhead. The client renders inside a system webview on three platforms;
less JS to parse means faster cold start on the weakest of them (WebView2 on
low-end Windows hardware). The marketing site carries a "total JS < 80 KB"
budget (docs/spec/04-marketing-site.md) — sharing one framework across product
surfaces only works if the framework fits under it.

**Tauri fit.** Svelte + Vite is a first-class `create-tauri-app` pairing;
`vite-plugin-svelte` needs no extra configuration to produce the static
`dist/` that `frontendDist` consumes, and the fixed-port Vite dev server maps
directly onto `devUrl`. No SSR framework (SvelteKit/Next) is wanted here —
the shell is a purely static bundle inside a webview, and plain Svelte + Vite
avoids adapter indirection.

**Ergonomics for this app's shape.** The desktop client is one long-lived
conversation surface with heavy streaming updates (tokens, tool cards,
provenance lines). Svelte 5 runes give fine-grained reactivity without
memoization ceremony — streaming state updates re-render only the node that
changed, which is exactly the workload §2 describes. Scoped `<style>` blocks
in single-file components keep component CSS next to markup while the token
layer stays global.

**Ecosystem risk, considered.** React's ecosystem is larger, but the spec
forbids most of what component libraries would provide (design-spec §1.6), and
the UI grammar is bespoke (milled ring, provenance line, tool cards) — we
would hand-roll components either way. Icons come from Lucide (§1.7), which
ships a first-party Svelte package.

## Tokens: CSS custom properties, not Tailwind (for now)

The token layer (`src/styles/tokens.css`) implements design-spec §1.3–1.5 as
native custom properties:

- The spec's own vocabulary is token-first (`paper`, `ink`, `signal`…), and
  §1.2's rule — "the `--signal` token may only be referenced by components on
  the allowed list" — is lintable as written when tokens are custom
  properties.
- Light/dark theming stays one `@media (prefers-color-scheme)` block instead
  of `dark:` variants scattered through markup.
- Zero build-time dependencies added for styling; the CSP stays
  `default-src 'self'` (fonts are vendored under `src/fonts/`, both SIL OFL).

Tailwind remains explicitly permitted by harness-spec §6. If utility classes
earn their keep once real layout work starts, Tailwind v4 themes are
themselves CSS custom properties, so adopting it later means mapping
`@theme` to this file — not rework.

## Consequences

- Later tickets build screens as `.svelte` components under `src/`;
  `src/index.html` is the Vite entry (the structure smoke asserts it).
- Frontend output lands in `dist/` (gitignored); `frontendDist` points at it.
- TypeScript is not wired in this slice; `vite-plugin-svelte` supports
  `lang="ts"` via esbuild if a later ticket wants it, with `svelte-check` as
  the checker.
- CI needs only node 24 + npm, unchanged (`.github/workflows/ci.yml`
  untouched).
