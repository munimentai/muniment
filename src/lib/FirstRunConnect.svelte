<script>
  import ProviderLogo from './ProviderLogo.svelte'
  import LucideIcon from './LucideIcon.svelte'
  import { SERVER_STATES, connectOptions } from './first-run-providers.js'

  // The first run's one question. Every option is a one-line button, found
  // ones first. More providers and Explore without connecting end the list. A
  // server that needs a step opens that step in place.
  let { scan = null, scanning = false, scanError = '', homePath = '', homeLoading = false, busy = false, onconnect, onmore, onexplore, onrescan } = $props()
  let open = $state(null)
  let key = $state('')
  const options = $derived(connectOptions(scan))
  const found = $derived(options.filter((option) => option.kind === 'server' || option.hint))
  const others = $derived(options.filter((option) => option.kind === 'subscription' && !option.hint))
  const home = $derived(homePath.replace(/^\/(Users|home)\/[^/]+/, '~').replace(/^[A-Z]:\\Users\\[^\\]+/, '~'))

  function choose(option) {
    if (option.kind === 'server' && option.server.state !== 'ready') {
      open = open === option.id ? null : option.id
      key = ''
      return
    }
    onconnect?.(option)
  }
</script>

<section class="connect" aria-labelledby="connect-title">
  <h1 id="connect-title">Connect a model</h1>
  <p class="lede">Muniment runs on a model you already have. Pick one to start.</p>

  {#if scanning}
    <p class="state" role="status">Looking for models on this computer…</p>
  {:else if scanError}
    <p class="state" role="alert">{scanError} <button type="button" class="link" onclick={onrescan}>Look again</button></p>
  {/if}

  {#snippet card(option)}
    {@const server = option.kind === 'server' ? option.server : null}
    {@const state = server ? SERVER_STATES[server.state] : null}
    <li class:expanded={open === option.id}>
      <button type="button" class="option" disabled={busy} aria-expanded={server && server.state !== 'ready' ? open === option.id : undefined} onclick={() => choose(option)}>
        <ProviderLogo color provider={option.logo} size={20} />
        <strong>{option.name}</strong>
        <span class="action" data-tone={state?.tone}>{state ? state.action : option.hint ? 'Connect' : 'Sign in'}<LucideIcon name="chevron-right" size={14} /></span>
      </button>
      {#if open === option.id && server?.state === 'needs-key'}
        <div class="step">
          <p>{server.name} asks for an API key.</p>
          {#if server.keyFile}
            <button type="button" class="primary" onclick={() => onconnect?.(option, { keyFrom: server.keyFile })}>Use the key in <span class="mono">{server.keyFile}</span></button>
            <p class="or">Or paste a key.</p>
          {/if}
          <form onsubmit={(event) => { event.preventDefault(); if (key.trim()) onconnect?.(option, { key: key.trim() }) }}>
            <input type="password" bind:value={key} placeholder="API key" aria-label={`${server.name} API key`} autocomplete="off" />
            <button type="submit" disabled={!key.trim()}>Connect</button>
          </form>
        </div>
      {:else if open === option.id && server?.state === 'not-running'}
        <div class="step">
          <p>Open {server.name} and start its local server.</p>
          <button type="button" onclick={onrescan}>Look again</button>
        </div>
      {/if}
    </li>
  {/snippet}

  {#if found.length}
    <h2>Found on this computer</h2>
    <ul class="options">{#each found as option (option.id)}{@render card(option)}{/each}</ul>
    <h2>Subscriptions</h2>
  {/if}
  <ul class="options">{#each others as option (option.id)}{@render card(option)}{/each}</ul>

  <div class="ends">
    <button type="button" class="more" disabled={busy} onclick={onmore}><LucideIcon name="plus" size={14} /><span>More providers</span></button>
    <button type="button" class="more" disabled={busy} onclick={onexplore}><span>Explore without connecting</span><LucideIcon name="arrow-right" size={14} /></button>
  </div>

  <p class="home">
    {#if home}{'Your files go to '}{/if}<span class:mono={!!home} data-testid="onboarding-home-path" data-home-path={homePath}>{home || (homeLoading ? 'Finding Home…' : 'Choose a folder for your files.')}</span>{#if home}.{/if}
  </p>
</section>

<style>
  .connect { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; overflow-y: auto; width: min(840px, 100%); margin: 0 auto; padding: 0 2px 32px; box-sizing: border-box; scrollbar-width: thin; }
  h1 { margin: 0; font: 600 var(--text-22) var(--font-heading); color: var(--ink); }
  .lede { margin: 6px 0 20px; color: var(--muted); font-size: var(--text-15); }
  h2 { margin: 20px 0 8px; color: var(--muted); font: var(--text-12) var(--font-mono); text-transform: uppercase; letter-spacing: .04em; font-weight: 400; }
  .state { margin: 0 0 12px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .options { display: grid; grid-template-columns: repeat(auto-fill, minmax(max(260px, calc((100% - 16px) / 3)), 1fr)); gap: 8px; margin: 0; padding: 0; list-style: none; }
  li { display: flex; flex-direction: column; min-width: 0; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); }
  li.expanded { grid-column: 1 / -1; }
  .option { display: grid; grid-template-columns: 20px minmax(0, 1fr) auto; align-items: center; column-gap: 10px; width: 100%; min-height: 44px; padding: 8px 12px; border: 0; border-radius: var(--radius-panel); background: none; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .option:hover:not(:disabled) { background: var(--faint); }
  .option:disabled, .more:disabled { cursor: default; }
  strong { min-width: 0; overflow: hidden; font-size: var(--text-15); font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
  .mono { font-family: var(--font-mono); }
  .action { justify-self: end; display: inline-flex; align-items: center; gap: 2px; color: var(--muted); font-size: var(--text-12); white-space: nowrap; }
  .option:hover .action { color: var(--ink); }
  .action[data-tone="ochre"] { color: var(--ochre); }
  .step { display: flex; flex-direction: column; align-items: flex-start; gap: 8px; padding: 0 12px 12px 42px; }
  .step p { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .step .or { margin-top: 4px; }
  form { display: flex; gap: 8px; width: 100%; }
  input { flex: 1; min-width: 0; min-height: 30px; padding: 4px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--paper); color: var(--ink); font: var(--text-13) var(--font-mono); }
  .step button, .more { min-height: 30px; padding: 5px 12px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); cursor: pointer; }
  .step button:hover:not(:disabled), .more:hover { background: var(--faint); }
  .step button:disabled { color: var(--muted); cursor: default; }
  .step .primary { background: var(--ink); border-color: var(--ink); color: var(--paper); }
  .ends { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 8px; margin-top: 12px; }
  .more { display: inline-flex; align-items: center; justify-content: center; gap: 6px; min-height: 44px; border-style: dashed; border-radius: var(--radius-panel); background: none; color: var(--muted); }
  .more:hover { color: var(--ink); }
  .home { margin: 24px 0 0; color: var(--muted); font-size: var(--text-12); }
  .link { padding: 0; border: 0; background: none; color: var(--ink); font: inherit; text-decoration: underline; text-underline-offset: 2px; cursor: pointer; }
</style>
