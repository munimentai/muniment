<script>
  // The Companies section of Settings: every company on this machine with
  // Open, Rename and Delete, and New company under the list. Delete asks
  // once and names the company, because it removes the graph with it.
  import { onMount } from 'svelte'

  let { tauri, onchanged = () => {} } = $props()

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

<section class="settings-companies" aria-labelledby="settings-companies-title">
  <h4 id="settings-companies-title" class="settings-label">Companies</h4>
  <p class="support">Each company is one record on this machine, its graph in its own file. Delete removes the company and every record in it.</p>
  {#if companies.length === 0}
    <p class="support">No company yet.</p>
  {:else}
    <ul class="company-list" aria-label="Companies">
      {#each companies as company (company.id)}
        <li class="company-row">
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
  <form class="company-new" aria-label="New company" onsubmit={create}>
    <input type="text" aria-label="New company name" placeholder="Company name" maxlength="120" bind:value={newName}>
    <button type="submit" disabled={busy || !newName.trim()}>New company</button>
  </form>
  {#if error}<p class="support error" role="alert">{error}</p>{/if}
</section>

<style>
  .settings-companies { display: grid; gap: 10px; justify-items: start; }
  .settings-label { margin: 0; color: var(--muted); font: var(--text-12) var(--font-mono); letter-spacing: .04em; text-transform: uppercase; }
  .support { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .error { color: var(--ink); }
  .company-list { width: 100%; max-width: 640px; margin: 0; padding: 0; list-style: none; border-top: 1px solid var(--border); }
  .company-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 36px; padding: 6px 0; border-bottom: 1px solid var(--border); font-size: var(--text-13); }
  .company-name { display: inline-flex; align-items: center; gap: 8px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .company-current { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .company-tools { display: inline-flex; flex: none; gap: 4px; }
  .company-rename, .company-new { display: flex; align-items: center; gap: 6px; width: 100%; max-width: 640px; }
  .company-rename { flex: 1; }
  input { flex: 1; min-width: 0; height: 28px; padding: 0 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); }
  input:focus { outline: none; border-color: var(--muted); }
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 4px 10px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { color: var(--muted); cursor: default; }
  .quiet { background: transparent; border-color: transparent; }
  .danger { border-color: var(--ochre, var(--border)); }
</style>
