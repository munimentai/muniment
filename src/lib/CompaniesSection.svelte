<script>
  // The Companies page of Settings: New company first, every company on this
  // machine as a card with Open, Rename and Delete, then the sources that fill
  // a company and the sample company, both in the record panel, and backups.
  // Delete asks once and names the company, because it removes the graph with it.
  import { onMount } from 'svelte'
  import RecordSources from '../record/RecordSources.svelte'
  import { sourceOptions } from '../record/record-import-state.js'
  import Toggle from './Toggle.svelte'
  import ChoiceField from './ui/ChoiceField.svelte'
  import { readDevicePreference, saveDevicePreference } from './device-preferences.js'

  let { tauri, onchanged = () => {}, onrecord = null } = $props()
  // Backups are a design ahead of the runtime: the choice stays on this device until the record backs itself up.
  let allSources = $state(false)
  let backups = $state(readDevicePreference('muniment.record-backups', { enabled: true, keep: '30' }))
  const keepOptions = [['7', '7 days'], ['30', '30 days'], ['90', '90 days']].map(([value, label]) => ({ value, label }))
  const initials = (name) => name.split(/\s+/).filter(Boolean).slice(0, 2).map((word) => word[0].toUpperCase()).join('') || '·'

  let companies = $state([])
  let error = $state(null)
  let busy = $state(false)
  let renaming = $state(null)
  let draft = $state('')
  let confirming = $state(null)
  let newName = $state('')

  function line(failure) {
    const text = typeof failure === 'string' ? failure : failure?.error?.message ?? failure?.message ?? String(failure ?? '')
    const first = text.split(/(?<=[.!?])\s/)[0]?.trim() || 'The record did not answer.'
    return first.endsWith('.') ? first : `${first}.`
  }

  async function load() {
    try {
      const listed = await tauri.invoke('record_companies')
      companies = Array.isArray(listed?.companies) ? listed.companies : []
    } catch (failure) {
      error = line(failure)
    }
  }

  onMount(() => { void load() })

  async function call(command, payload) {
    busy = true
    error = null
    try {
      const answer = await tauri.invoke(command, payload)
      if (answer?.error) {
        error = line(answer)
        return false
      }
      await load()
      onchanged()
      return true
    } catch (failure) {
      error = line(failure)
      return false
    } finally {
      busy = false
    }
  }

  async function create(event) {
    event?.preventDefault?.()
    const name = newName.trim()
    if (!name) return
    if (await call('record_company_create', { name })) newName = ''
  }

  async function rename(event, id) {
    event?.preventDefault?.()
    const name = draft.trim()
    if (!name) return
    if (await call('record_company_rename', { companyId: id, name })) renaming = null
  }

  async function remove(id) {
    if (confirming !== id) {
      confirming = id
      return
    }
    if (await call('record_company_delete', { companyId: id })) confirming = null
  }
</script>

<section class="settings-companies" aria-label="Companies">
  <p class="settings-intro">Company records stay on this device. Deleting a company removes its records.</p>
  <form class="company-new" aria-label="New company" onsubmit={create}>
    <input type="text" aria-label="New company name" placeholder="Company name" maxlength="120" bind:value={newName}>
    <button type="submit" disabled={busy || !newName.trim()}>New company</button>
  </form>
  {#if error}<p class="support error" role="alert">{error}</p>{/if}

  <section class="companies-block" aria-labelledby="companies-heading">
    <h3 id="companies-heading" class="settings-heading">Your companies</h3>
    {#if companies.length === 0}
      <p class="support">No company yet.</p>
    {:else}
      <ul class="company-list" aria-label="Companies">
        {#each companies as company (company.id)}
          <li class="company-row" class:current={company.current}>
            {#if renaming === company.id}
              <form class="company-rename" aria-label="Rename {company.name}" onsubmit={(event) => rename(event, company.id)}>
                <input type="text" aria-label="Company name" maxlength="120" bind:value={draft}>
                <button type="submit" disabled={busy || !draft.trim()}>Save</button>
                <button type="button" class="quiet" onclick={() => { renaming = null }}>Cancel</button>
              </form>
            {:else if confirming === company.id}
              <span class="company-name">Delete {company.name} and its records?</span>
              <span class="company-tools">
                <button type="button" class="danger" disabled={busy} onclick={() => remove(company.id)}>Delete</button>
                <button type="button" class="quiet" onclick={() => { confirming = null }}>Keep</button>
              </span>
            {:else}
              <span class="company-mark" aria-hidden="true">{initials(company.name)}</span>
              <span class="company-name">{company.name}{#if company.current}<span class="company-current">current</span>{/if}</span>
              <span class="company-tools">
                {#if !company.current}<button type="button" class="quiet" disabled={busy} onclick={() => call('record_company_select', { companyId: company.id })}>Open</button>{/if}
                <button type="button" class="quiet" disabled={busy} onclick={() => { draft = company.name; renaming = company.id; confirming = null }}>Rename</button>
                <button type="button" class="quiet" disabled={busy} onclick={() => remove(company.id)}>Delete</button>
              </span>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  {#if onrecord}
    <section class="companies-block" aria-labelledby="sources-heading">
      <h3 id="sources-heading" class="settings-heading">Sources</h3>
      <p class="support">Records arrive from the tools a company already uses. Pick one to connect it in the record panel.</p>
      <RecordSources onconnect={(source) => onrecord({ source })} limit={allSources ? Infinity : 8} />
      {#if !allSources}<button type="button" class="quiet more" onclick={() => { allSources = true }}>Show all {sourceOptions().length} sources</button>{/if}
    </section>

    <section class="companies-block" aria-labelledby="sample-heading">
      <h3 id="sample-heading" class="settings-heading">Sample company</h3>
      <div class="sample-card">
        <span class="company-mark" aria-hidden="true">NT</span>
        <div><strong>Northwind Traders</strong><p class="support">A filled-in company with its findings and their evidence, to explore before you add your own.</p></div>
        <button type="button" onclick={() => onrecord({ sample: true })}>Open the sample</button>
      </div>
    </section>
  {/if}

  <section class="companies-block" aria-labelledby="backups-heading">
    <h3 id="backups-heading" class="settings-heading">Backups</h3>
    <div class="backup-rows">
      <div class="backup-row"><div><strong>Back up company records daily</strong><p class="support">A copy of every company stays beside the record on this device.</p></div><Toggle checked={backups.enabled} label="Back up company records daily" onchange={(enabled) => { backups = saveDevicePreference('muniment.record-backups', { ...backups, enabled }) }} /></div>
      {#if backups.enabled}<div class="backup-row"><ChoiceField label="Keep backups for" value={backups.keep} options={keepOptions} onchange={(keep) => { backups = saveDevicePreference('muniment.record-backups', { ...backups, keep }) }} /></div>{/if}
    </div>
  </section>
</section>

<style>
  /* Companies reads like every Settings page: the intro, New company as the
     toolbar, then sections under semibold headings with cards, not dividers. */
  .settings-companies { display: grid; gap: 28px; max-width: 760px; }
  .settings-companies > .settings-intro { margin-bottom: -14px; }
  .companies-block { display: grid; gap: 10px; }
  .companies-block > h3 { margin: 0; }
  .support { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .error { color: var(--ink); }
  .company-list { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 10px; margin: 0; padding: 0; list-style: none; }
  .company-row { display: flex; align-items: center; gap: 12px; min-width: 0; min-height: 44px; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); font-size: var(--text-13); }
  .company-row.current { background: var(--faint); }
  .company-mark { flex: none; display: grid; place-items: center; width: 36px; height: 36px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); font: var(--weight-semibold) var(--text-13) var(--font-human); }
  .company-name { flex: 1; display: inline-flex; align-items: center; gap: 8px; min-width: 0; overflow: hidden; white-space: nowrap; font-weight: 600; mask-image: linear-gradient(to right, currentColor calc(100% - 28px), transparent); }
  .company-current { color: var(--muted); font: var(--weight-regular) var(--text-12) var(--font-mono); }
  .company-tools { display: inline-flex; flex: none; gap: 2px; }
  .company-rename, .company-new { display: flex; align-items: center; gap: 8px; width: 100%; }
  .company-rename { flex: 1; }
  .sample-card { display: flex; align-items: center; gap: 12px; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .sample-card > div { flex: 1; display: grid; gap: 2px; min-width: 0; }
  .sample-card strong, .backup-row strong { font-size: var(--text-13); font-weight: 600; }
  .backup-rows { display: grid; padding: 0 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .backup-row { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 12px 0; }
  .backup-row + .backup-row { border-top: 1px solid var(--border); }
  .backup-row > div { display: grid; gap: 2px; }
  input { flex: 1; min-width: 0; height: 32px; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); }
  input:focus { outline: none; border-color: var(--muted); }
  button { flex: none; font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--faint); border: 1px solid transparent; border-radius: var(--radius-control); padding: 6px 10px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--border); }
  button:disabled { color: var(--muted); cursor: default; }
  .quiet { background: transparent; }
  .more { justify-self: start; color: var(--muted); }
  .quiet:hover:not(:disabled) { background: var(--faint); }
  .danger { border-color: var(--ochre, var(--border)); }
</style>
