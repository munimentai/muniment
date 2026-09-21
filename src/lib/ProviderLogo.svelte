<script>
  // A provider's real mark, vendored as SVG. A provider with a light and a dark
  // variant shows the one for the current theme; the rest are one file.
  const files = import.meta.glob('./logos/*.svg', { query: '?raw', import: 'default', eager: true })
  const logos = {}
  for (const [path, svg] of Object.entries(files)) {
    const name = path.slice('./logos/'.length, -'.svg'.length)
    const match = name.match(/^(.*)-(light|dark)$/)
    if (match) (logos[match[1]] ??= {})[match[2]] = svg
    else logos[name] = { any: svg }
  }
  // Pi ids that share a mark with the provider the user connected.
  const ALIASES = { 'jev-latest': 'typesafe', 'openai-codex': 'openai', 'claude-bridge': 'anthropic', 'opencode-go': 'opencode', 'qwen-token-plan-individual': 'qwen-token-plan', 'minimax-cn': 'minimax', 'zai-coding-cn': 'zai', kimi: 'kimi-coding' }

  let { provider, size = 16 } = $props()
  const logo = $derived(logos[ALIASES[provider] ?? provider?.replace(/^custom-.*/, 'custom')] ?? null)
</script>

{#if logo?.any}
  <span class="logo" style:width="{size}px" style:height="{size}px" aria-hidden="true">{@html logo.any}</span>
{:else if logo}
  <span class="logo light" style:width="{size}px" style:height="{size}px" aria-hidden="true">{@html logo.light ?? logo.dark}</span>
  <span class="logo dark" style:width="{size}px" style:height="{size}px" aria-hidden="true">{@html logo.dark ?? logo.light}</span>
{:else}
  <span class="logo blank" style:width="{size}px" style:height="{size}px" aria-hidden="true"></span>
{/if}

<style>
  .logo { display: inline-flex; flex: none; align-items: center; justify-content: center; overflow: hidden; }
  .logo :global(svg) { width: 100%; height: 100%; }
  .blank { border: 1px solid var(--border); border-radius: var(--radius-chip); }
  .dark { display: none; }
  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-scheme="light"])) .light { display: none; }
    :global(:root:not([data-scheme="light"])) .dark { display: inline-flex; }
  }
  :global(:root[data-scheme="dark"]) .light { display: none; }
  :global(:root[data-scheme="dark"]) .dark { display: inline-flex; }
</style>
