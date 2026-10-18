<script>
  import PopupClose from './PopupClose.svelte'
  import { contextLevel, contextShare, formatTokens } from './context-usage.js'
  // The Context panel over the composer: how full the conversation is, where
  // compaction runs, the model and its window, and Compact now.
  let { usage, active = false, compacting = false, error = '', oncompact, onsettings, onclose } = $props()
  const level = $derived(contextLevel(usage))
  const share = $derived(contextShare(usage))
  const tick = $derived(usage?.compactAt && usage?.contextWindow ? Math.min(usage.compactAt / usage.contextWindow, 1) : null)
  const disabled = $derived(active || compacting)
  function submit(event) {
    event.preventDefault()
    if (!disabled) oncompact(null)
  }
</script>
<div data-panel="context" data-panel-variant="overlay" data-composer-panel class="context" role="dialog" aria-label="Conversation context">
  <header><strong>Context</strong><PopupClose label="Close context" onclick={onclose} /></header>
  {#if usage?.tokens != null}
    <p class="count">{usage.estimated ? 'About ' : ''}{formatTokens(usage.tokens)}{#if usage.contextWindow}<small>of {formatTokens(usage.contextWindow)} tokens{usage.windowKnown ? '' : ' (assumed)'}</small>{:else}<small>tokens</small>{/if}</p>
  {:else}
    <p class="count"><small>Context usage appears after the next reply.</small></p>
  {/if}
  <div class="bar" data-level={level} aria-hidden="true"><i style:width={`${share * 100}%`}></i>{#if tick != null}<b style:left={`${tick * 100}%`}></b>{/if}</div>
  {#if usage && !usage.windowKnown}
    <p class="caution">The model reports no context window{usage?.model ? ` for ${usage.model}` : ''}. Its server may drop older messages without warning.</p>
  {:else if usage && !usage.autoCompact}
    <p class="caption">Automatic compaction is off.</p>
  {:else if usage?.compactAt}
    <p class="caption">Compacts automatically at {formatTokens(usage.compactAt)} tokens.</p>
  {/if}
  {#if usage?.model}
    <dl>
      <div><dt>Model</dt><dd>{usage.model}</dd></div>
      {#if usage.contextWindow}<div><dt>Window</dt><dd>{formatTokens(usage.contextWindow)}</dd></div>{/if}
    </dl>
  {/if}
  <form onsubmit={submit}>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if active}<p class="caption" role="status">Compact after this reply finishes.</p>{/if}
    <footer>
      <button type="button" class="link" onclick={() => { onclose(); onsettings() }}>Context settings</button>
      <button type="submit" class="primary" {disabled}>{compacting ? 'Compacting…' : 'Compact now'}</button>
    </footer>
  </form>
</div>
<style>
  .context { width: min(340px, 100%); box-sizing: border-box; padding: var(--panel-control-inset); font: var(--text-12) var(--font-human); }
  header, footer { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  header { margin-bottom: 10px; }
  .count { margin: 0 0 6px; color: var(--ink); font: var(--text-13) var(--font-mono); }
  .count small { margin-left: 1ch; color: var(--muted); font-size: var(--text-12); }
  .bar { position: relative; height: 6px; margin: 0 0 6px; border: 1px solid var(--border); border-radius: var(--radius-chip); background: var(--faint); }
  .bar i { position: absolute; inset: 0 auto 0 0; border-radius: var(--radius-chip); background: var(--muted); }
  .bar[data-level="caution"] i { background: var(--ochre); }
  .bar b { position: absolute; top: -4px; bottom: -4px; width: 1px; background: var(--ink); }
  p, .caption { margin: 6px 0; color: var(--muted); }
  .caution { color: var(--ochre); }
  .error { color: var(--ink); font-family: var(--font-mono); }
  dl { display: grid; gap: 4px; margin: 10px 0; }
  dl div { display: flex; justify-content: space-between; gap: 12px; }
  dt { color: var(--muted); }
  dd { margin: 0; font-family: var(--font-mono); overflow-wrap: anywhere; text-align: right; }
  footer { margin-top: 12px; }
  button { min-height: 24px; padding: 4px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: transparent; color: var(--ink); font: inherit; cursor: pointer; }
  button:disabled { color: var(--muted); cursor: default; }
  .primary { background: var(--accent); border-color: var(--accent); color: var(--on-accent); }
  .primary:disabled { background: var(--faint); border-color: var(--border); }
  .link { border: 0; padding: 0; color: var(--muted); text-decoration: underline; text-underline-offset: 2px; }
</style>
