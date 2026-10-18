<script>
  import LucideIcon from './LucideIcon.svelte'
  import { CLASSIFIERS } from './classifier-connections.js'
  // A provider's real mark, vendored as SVG. A provider with a light and a dark
  // variant shows the one for the current theme; the rest are one file.
  const files = import.meta.glob('./logos/*.svg', { query: '?raw', import: 'default', eager: true })
  // Nimble uses Bespoke Labs’ favicon: https://framerusercontent.com/images/tBmIC2QpuzBT566Qth8NNaI5CJE.png
  // oMLX uses its site icon, images/icon-rounded-dark.svg, without its drop shadow.
  const images = import.meta.glob('./logos/*.{webp,png}', { query: '?url', import: 'default', eager: true })
  const classifiers = new Set(CLASSIFIERS.map(entry => entry.id))
  const logos = {}
  for (const [path, svg] of Object.entries(files)) {
    const name = path.slice('./logos/'.length, -'.svg'.length)
    const match = name.match(/^(.*)-(light|dark)$/)
    if (match) (logos[match[1]] ??= {})[match[2]] = svg
    else logos[name] = { any: svg }
  }
  // Pi ids that share a mark with the provider the user connected.
  const ALIASES = {
    jev: 'typesafe', 'jev-latest': 'typesafe', clef: 'cloudflare', luna: 'openai',
    'openai-codex': 'openai', 'claude-bridge': 'anthropic', antigravity: 'google',
    'opencode-go': 'opencode', kimi: 'kimi-coding',
    'qwen-token-plan-individual': 'qwen-token-plan', 'qwen-token-plan-cn': 'qwen-token-plan',
    'minimax-cn': 'minimax', 'zai-coding-cn': 'zai', 'moonshotai-cn': 'moonshotai',
    'xiaomi-token-plan-sgp': 'xiaomi', 'xiaomi-token-plan-cn': 'xiaomi', 'xiaomi-token-plan-ams': 'xiaomi',
    'google-vertex': 'google', 'cloudflare-ai-gateway': 'cloudflare',
    'cloudflare-workers-ai': 'cloudflare-workers', 'vercel-ai-gateway': 'vercel',
  }

  // A provider whose own mark is one color shows its brand's colored mark
  // where `color` asks for it: the first run and the model Settings pages.
  const COLOR_MARKS = { anthropic: 'claude' }

  // Simple Icons marks ship without a fill, so they take the text color.
  const UNFILLED = new Set(['search-wikipedia', 'search-startpage'])

  // A custom endpoint takes the generic mark, except one a server kind names,
  // such as the oMLX endpoint the local server scan connects as custom-omlx.
  const CUSTOM_MARKS = { omlx: 'omlx' }
  const customMark = (value) => CUSTOM_MARKS[value?.match(/^custom-(.+?)(?:-\d+)?$/)?.[1]] ?? 'custom'

  let { provider, size = 16, color = false } = $props()
  const id = $derived(ALIASES[provider] ?? (provider?.startsWith('custom-') ? customMark(provider) : provider))
  const colorId = $derived(color ? `${COLOR_MARKS[id] ?? id}-color` : null)
  const logo = $derived(logos[colorId] ?? logos[id] ?? null)
  const image = $derived(images[`./logos/${id}.webp`] ?? images[`./logos/${id}.png`] ?? null)
</script>

{#if logo?.any}
  <span class="logo" class:unfilled={UNFILLED.has(id)} style:width="{size}px" style:height="{size}px" aria-hidden="true">{@html logo.any}</span>
{:else if logo}
  <span class="logo light" style:width="{size}px" style:height="{size}px" aria-hidden="true">{@html logo.light ?? logo.dark}</span>
  <span class="logo dark" style:width="{size}px" style:height="{size}px" aria-hidden="true">{@html logo.dark ?? logo.light}</span>
{:else if image}
  <span class="logo" style:width="{size}px" style:height="{size}px" aria-hidden="true"><img src={image} alt="" width={size} height={size} /></span>
{:else}
  <span class="logo fallback" style:width="{size}px" style:height="{size}px" aria-hidden="true"><LucideIcon name={classifiers.has(provider) ? 'route' : 'cpu'} {size} variant="action" /></span>
{/if}

<style>
  .logo { display: inline-flex; flex: none; align-items: center; justify-content: center; overflow: hidden; }
  .logo :global(svg) { width: 100%; height: 100%; }
  .logo img { width: 100%; height: 100%; object-fit: contain; }
  .fallback { color: var(--muted); }
  .unfilled { color: var(--ink); }
  .unfilled :global(svg) { fill: currentColor; }
  .dark { display: none; }
  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-scheme="light"])) .light { display: none; }
    :global(:root:not([data-scheme="light"])) .dark { display: inline-flex; }
  }
  :global(:root[data-scheme="dark"]) .light { display: none; }
  :global(:root[data-scheme="dark"]) .dark { display: inline-flex; }
</style>
