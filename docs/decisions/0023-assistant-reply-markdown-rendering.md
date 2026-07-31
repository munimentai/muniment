# 0023 — Assistant reply Markdown rendering

- Status: accepted
- Date: 2026-07-30
- Context: desktop-app spec §§2.2 and 3.1; ADRs 0018 and 0020

## Context

Pi writes Markdown, but the desktop shows its source characters. At
`src/App.svelte:727`, a `<p class="response-prose">` displays
`message.run.text`. At `src/App.svelte:1072`, `.response-prose` only sets
`white-space: pre-wrap`. No file under `src/` parses Markdown.

The 2026-07-30 planner render sent a Markdown reply through the built bundle
at 1100 by 720 pixels. Heading marks, bullets, emphasis marks, inline
backticks, fences, and pipe-table rows appeared verbatim in sans-serif prose.

Desktop-app spec §3.1 calls a response left-aligned plain text on `paper`.
That rule defines the visual register. It does not require visible Markdown
source. ADR 0018 treats model output as untrusted content, and
`THREAT_MODEL.md` says model output is not trustworthy.

Desktop-app spec §2.2 requires a signal underline and caret during streaming.
`src/lib/streaming-underline.js` measures the caret's last inline fragment.
Markdown blocks would change that geometry during a run.

### Renderer bars

The parser must cover CommonMark and the useful GFM extensions. It must expose
tokens so the application can enforce a small element subset. It must not
render raw HTML. It must define partial-syntax behavior. It must fit Svelte 5
without React and use a permissive license.

The dependency counts below cover direct runtime dependencies. Bundle sizes
are minified and gzip-compressed Bundlephobia measurements from 2026-07-30.
The selected sanitizer adds 10.4 kB gzip and no direct dependency.

| Candidate | CommonMark and GFM coverage | Dependencies and added bundle size | Raw-HTML stance | Partial syntax while streaming | Svelte 5 fit | License |
| --- | --- | --- | --- | --- | --- | --- |
| `marked` 18.0.7 | Its published suites report 98% CommonMark and 97% GFM. Tables, task lists, and strikethrough are built in. | It has zero direct dependencies and adds 12.5 kB gzip. | It emits raw HTML unless a token renderer replaces HTML tokens. DOMPurify must sanitize the final output. | It parses the accumulated text as a complete document. Open constructs can change earlier output. | Its framework-neutral token API fits Svelte 5. | It uses MIT. |
| `markdown-it` 15.0.0 | It reports CommonMark compliance. Tables and strikethrough are built in, while other GFM features need plugins. | It has six direct dependencies and adds 47.0 kB gzip before plugins. | Its `html: false` option escapes raw HTML. A sanitizer must still guard the DOM boundary. | It reparses the complete accumulated document. Open constructs can change earlier output. | Its framework-neutral renderer fits Svelte 5. | It uses MIT. |
| `micromark` 4.0.2 plus `micromark-extension-gfm` | The core targets CommonMark. The extension adds GFM autolinks, footnotes, tables, task lists, and strikethrough. | The core has 17 direct dependencies and adds 14.6 kB gzip. The GFM extension adds eight direct dependencies. | HTML is encoded by default. Custom HTML output still needs sanitization. | Its streaming tokenizer can accept chunks, but safe DOM output needs a custom token-to-node layer. | Its framework-neutral events fit Svelte 5, but the application must build a renderer. | Both packages use MIT. |
| `Streamdown` 2.5.0 | Its unified pipeline uses `remark-gfm` and handles common assistant Markdown. | It has 16 direct dependencies. Its entry chunks alone add 21.6 kB gzip before React, Mermaid, and other dependencies. | It uses `rehype-harden` and `rehype-sanitize` with configurable link and image rules. | `remend` repairs open emphasis, fences, links, and other partial constructs during streaming. | Its published component requires React and a React JSX runtime. | It uses Apache-2.0. |

## Decision

### Parser and element subset

The desktop will use `marked` with a local token renderer and DOMPurify.
The pair adds two direct dependencies and about 22.9 kB gzip. It covers the
observed reply without adding React or another component system.

The renderer accepts these Markdown constructs:

- Paragraphs and soft or hard line breaks.
- Heading levels one through four, mapped to HTML `h2` through `h5`.
- Ordered lists, unordered lists, and nested list items.
- Strong emphasis, emphasis, and strikethrough.
- Inline code and fenced code blocks.
- Block quotes and thematic breaks.
- Links with approved URL schemes.
- GFM tables with a header and body.

The resulting element allowlist is `a`, `blockquote`, `br`, `code`, `del`,
`em`, `h2`, `h3`, `h4`, `h5`, `hr`, `li`, `ol`, `p`, `pre`, `strong`,
`table`, `tbody`, `td`, `th`, `thead`, `tr`, and `ul`. Links may have only
`href`, `title`, `target`, and `rel`. Code may have only a validated
`language-*` class. Table alignment does not create a style attribute.

Unless a stricter rule follows, every construct outside the subset renders as
literal source text. This rule includes headings below level four, task
checkboxes, footnotes, definitions, and extension tokens. Raw HTML also
renders as literal source text and never creates an element.

Images never create an `img` element. A remote image renders its alt text as
plain text, without a request. The same rule covers local and data URLs.

A link may use only `https:`, `http:`, or `mailto:`. A relative link is not
allowed because a reply has no trusted base URL. An allowed link opens outside
the webview and gets `target="_blank"` plus `rel="noopener noreferrer"`.
A missing, malformed, or disallowed URL renders only its label. Therefore, a
`javascript:` link cannot navigate or create an anchor.

### Sanitization boundary

ADR 0018 and `THREAT_MODEL.md` place model output outside the runtime trust
boundary. The complete terminal reply remains untrusted through parsing.
The local token renderer first rejects unsupported constructs and URLs.
DOMPurify then sanitizes the generated HTML with the exact element and
attribute allowlists above.

Only DOMPurify's returned value may cross the HTML insertion boundary.
No caller may insert parser output, token text, or model text through
`innerHTML` or Svelte `{@html}` before this step. If DOMPurify reports any
removal, the renderer discards all rich output and shows the complete original
reply through a text node. It also records a local diagnostic without the
reply text. Empty replies produce an empty text node.

This fallback avoids displaying a changed version as though the model wrote
it. It also covers malformed attributes, contradictory parser output, and a
future token type that lacks a reviewed rule. The Tauri CSP at
`src-tauri/tauri.conf.json:24` remains a backup defense. CSP does not replace
the token rules, URL filter, or sanitizer.

### Streaming interaction

Markdown renders only after the terminal run event. While a run streams, the
desktop appends the accumulated reply as plain text and preserves source
characters. It does not parse isolated chunks or repair incomplete syntax.

During streaming, `src/lib/streaming-underline.js` keeps measuring the caret
after the last plain-text inline fragment. The signal rule spans that active
line under design-spec §2.2. The terminal event removes the caret and signal
rule, then replaces the plain-text node with one sanitized Markdown tree.
Block layout never participates in active-line measurement.

This choice avoids unstable heading, list, table, and fence geometry. It also
sanitizes the combined output, so an unsafe construct cannot hide across
chunks. A canceled or failed run has a terminal state and renders its complete
accumulated text through the same boundary.

### Visual register and ADR 0020

The response remains on `--paper` with `--ink` prose and no container.
Inline code uses `--font-mono` with a `--faint` background. Fenced code uses
`--font-mono`, `--faint`, and a `--border` edge. Links use `--ink` with an
underline. Hover uses `--muted`, and keyboard focus uses an `--ink` outline.
The Markdown register does not use `--signal`, which remains reserved for
streaming and routes.

Fenced code stays plain until the curated syntax highlighter from ADR 0020
lands. Markdown code styling and the diff token override sheet share the same
code tokens. A `code-diff/1` value remains an application-owned diff and uses
the `diff2html` path. A fenced `diff` in model prose remains an inert code
block and never becomes an approvable diff.

### Implementation slices

The first implementation slice adds the exact `marked` and DOMPurify pins.
It adds `src/lib/assistant-markdown.js` and its invalid-input tests. It adds
`src/lib/AssistantMarkdown.svelte` and a local token stylesheet. It updates
`src/App.svelte` only in that later slice.

That slice tests empty text, every allowed element, and every unsupported
construct. It tests raw HTML, remote images, malformed URLs, `javascript:`
links, sanitizer removals, incomplete Markdown, cancellation, and failure.
It also tests the plain-text-to-terminal-render transition and underline
removal.

A later syntax slice may reuse ADR 0020's curated highlighter. It must keep
the Markdown allowlist and sanitizer boundary unchanged.

### Sources

- [`marked` documentation](https://marked.js.org/)
- [`markdown-it` documentation](https://github.com/markdown-it/markdown-it)
- [`micromark` documentation](https://github.com/micromark/micromark)
- [`Streamdown` documentation](https://streamdown.ai/docs)
- [Chrome streamed model-output guidance](https://developer.chrome.com/docs/ai/render-llm-responses)
- [HackerOne Markdown rendering guidance](https://www.hackerone.com/blog/secure-markdown-rendering-react-balancing-flexibility-and-safety)
- [`DOMPurify` documentation](https://github.com/cure53/DOMPurify)
- [Bundlephobia package measurements](https://bundlephobia.com/)

## Consequences

- Assistant replies show common Markdown after their run reaches a terminal state.
- Streaming keeps the existing plain-text underline geometry.
- The parser, token renderer, URL filter, and DOM sanitizer form one reviewable boundary.
- Unsupported constructs remain visible without creating unreviewed elements or network requests.
- The first implementation slice adds two framework-neutral dependencies.
- This decision changes no runtime code or dependency manifest.
