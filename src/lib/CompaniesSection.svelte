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
  import PageHead from './ui/PageHead.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import Button from './ui/Button.svelte'
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
  <PageHead intro="Company records stay on this device. Deleting a company removes its records." />
  {#if error}<p class="support error" role="alert">{error}</p>{/if}

  <Group heading="Your companies">
    {#each companies as company (company.id)}
      {#if renaming === company.id}
        <form class="company-form" aria-label="Rename {company.name}" onsubmit={(event) => rename(event, company.id)}>
          <input type="text" aria-label="Company name" maxlength="120" bind:value={draft}>
          <Button type="submit" disabled={busy || !draft.trim()}>Save</Button>
          <button type="button" class="quiet" onclick={() => { renaming = null }}>Cancel</button>
        </form>
      {:else if confirming === company.id}
        <Row title="Delete {company.name} and its records?">
          <Button disabled={busy} onclick={() => remove(company.id)}>Delete</Button>
          <button type="button" class="quiet" onclick={() => { confirming = null }}>Keep</button>
        </Row>
      {:else}
        <Row title={company.name} meta={company.current ? 'current' : ''}>
          {#snippet lead()}<span class="company-mark" aria-hidden="true">{initials(company.name)}</span>{/snippet}
          {#if !company.current}<button type="button" class="quiet" disabled={busy} onclick={() => call('record_company_select', { companyId: company.id })}>Open</button>{/if}
          <button type="button" class="quiet" disabled={busy} onclick={() => { draft = company.name; renaming = company.id; confirming = null }}>Rename</button>
          <button type="button" class="quiet" disabled={busy} onclick={() => remove(company.id)}>Delete</button>
        </Row>
      {/if}
    {:else}
      <p>No company yet.</p>
    {/each}
    <form class="company-form" aria-label="New company" onsubmit={create}>
      <input type="text" aria-label="New company name" placeholder="Company name" maxlength="120" bind:value={newName}>
      <Button type="submit" icon="plus" disabled={busy || !newName.trim()}>New company</Button>
    </form>
  </Group>

  {#if onrecord}
    <Group heading="Sources" description="Records arrive from the tools a company already uses. Pick one to connect it in the record panel." plain>
      <RecordSources onconnect={(source) => onrecord({ source })} limit={allSources ? Infinity : 8} />
      {#if !allSources}<button type="button" class="quiet more" onclick={() => { allSources = true }}>Show all {sourceOptions().length} sources</button>{/if}
    </Group>

    <Group heading="Sample company">
      <Row title="Northwind Traders" description="A filled-in company with its findings and their evidence, to explore before you add your own.">
        {#snippet lead()}<span class="company-mark" aria-hidden="true">NT</span>{/snippet}
        <Button onclick={() => onrecord({ sample: true })}>Open the sample</Button>
      </Row>
    </Group>
  {/if}

  <Group heading="Backups">
    <Row title="Back up company records daily" description="A copy of every company stays beside the record on this device.">
      <Toggle checked={backups.enabled} label="Back up company records daily" onchange={(enabled) => { backups = saveDevicePreference('muniment.record-backups', { ...backups, enabled }) }} />
    </Row>
    {#if backups.enabled}
      <Row title="Keep backups for" description="Older copies are removed.">
        <div class="choice"><ChoiceField label="Keep backups for" hideLabel value={backups.keep} options={keepOptions} onchange={(keep) => { backups = saveDevicePreference('muniment.record-backups', { ...backups, keep }) }} /></div>
      </Row>
    {/if}
  </Group>
</section>

<style>
  /* Companies builds from PageHead, Group and Row like every Settings page. */
  .settings-companies { display: grid; gap: var(--page-gap); }
  .support { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .error { color: var(--ink); }
  .company-mark { display: grid; place-items: center; width: 32px; height: 32px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); font: var(--weight-semibold) var(--text-12) var(--font-human); }
  .company-form { display: flex; align-items: center; gap: 8px; padding: 10px 0; }
  .choice { width: 220px; max-width: 100%; }
  input { flex: 1; min-width: 0; height: 32px; box-sizing: border-box; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); }
  input:focus { outline: none; border-color: var(--muted); }
  button { flex: none; min-height: 32px; font: inherit; font-size: var(--text-13); color: var(--ink); background: transparent; border: 1px solid transparent; border-radius: var(--radius-control); padding: 6px 10px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { color: var(--muted); cursor: default; }
  .quiet { background: transparent; }
  .more { justify-self: start; color: var(--muted); }
</style>
