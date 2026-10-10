<script>
  import { onMount } from 'svelte'
  import { parseProfile, serializeProfile } from './profile-fields.js'
  import Toggle from './Toggle.svelte'
  import { readDevicePreference, saveDevicePreference } from './device-preferences.js'
  // `part` shows one Settings page: profile, memory, or both for an agent.
  let { tauri, agentId = null, part = 'all' } = $props()
  const showProfile = $derived(!agentId && part !== 'memory')
  const showMemory = $derived(part !== 'profile')
  // Recall is a design ahead of the runtime: these choices stay on this device until recall reads them.
  let recall = $state(readDevicePreference('muniment.memory-recall', { learn: true, use: true, ask: true }))
  const RECALL = [
    ['learn', 'Save facts from chats', 'A chat proposes a memory when it states something lasting about you or your work.'],
    ['ask', 'Ask before saving', 'Proposed memories wait for your approval in the chat.'],
    ['use', 'Use memories in replies', 'Muniment recalls the saved facts that fit each message.'],
  ]
  let titleInput = $state()
  function exportMemories() {
    const text = facts.map((fact) => `## ${fact.title}\n\n${fact.content}\n\nSource: ${fact.source}\n`).join('\n')
    const url = URL.createObjectURL(new Blob([`# Saved memories\n\n${text}`], { type: 'text/markdown' }))
    Object.assign(document.createElement('a'), { href: url, download: 'memories.md' }).click()
    setTimeout(() => URL.revokeObjectURL(url), 0)
  }
  const memory = { invoke: (action, args = {}) => agentId ? tauri.invoke("agent_memory", { id: agentId, action, fact: args.fact, factId: args.id }) : tauri.invoke(action, args) }
  let profile = $state(parseProfile())
  let facts = $state([])
  let filter = $state('')
  let showDeleted = $state(false)
  let deletedFacts = $state([])
  let draft = $state({ id: '', title: '', content: '', source: 'Added in settings' })
  let status = $state('')
  let busy = $state(true)
  let loaded = $state(false)
  const shown = $derived(facts.filter(f => `${f.title} ${f.content} ${f.source}`.toLowerCase().includes(filter.toLowerCase())))
  onMount(async () => {
    try {
      const [text, rows] = await Promise.all([agentId ? Promise.resolve('') : memory.invoke('memory_profile_read'), memory.invoke('memory_facts')])
      profile = parseProfile(text)
      facts = rows
      loaded = true
    } catch (e) { status = String(e) }
    finally { busy = false }
  })
  async function saveProfile() {
    busy = true
    try { await memory.invoke('memory_profile_save', { content: serializeProfile(profile) }); status = 'Profile saved. New replies use these preferences.' }
    catch (e) { status = String(e) }
    finally { busy = false }
  }
  async function importProfile(event) {
    const file = event.target.files?.[0]
    if (!file) return
    if (file.size > 65536) { status = 'Choose a file under 64 KB.'; return }
    try { profile = parseProfile(await file.text()); status = 'Profile loaded. Save to use it in new replies.' }
    catch { status = 'The profile file could not be read.' }
    event.target.value = ''
  }
  async function saveFact(event) {
    event.preventDefault()
    busy = true
    try {
      await memory.invoke('memory_fact_save', { fact: { ...draft } })
      facts = await memory.invoke('memory_facts')
      draft = { id: '', title: '', content: '', source: 'Added in settings' }
      status = 'Memory saved.'
    } catch (e) { status = String(e) }
    finally { busy = false }
  }
  let deleting = $state('')
  async function refreshDeleted() {
    try { deletedFacts = await memory.invoke('memory_deleted_facts') || [] }
    catch (e) { status = String(e) }
  }
  async function restoreFact(id) {
    busy = true
    try {
      await memory.invoke('memory_fact_restore', { id })
      facts = await memory.invoke('memory_facts')
      await refreshDeleted()
      status = 'Memory restored.'
    } catch (e) { status = String(e) }
    finally { busy = false }
  }
  async function removeFact(id) {
    busy = true
    try { await memory.invoke('memory_fact_delete', { id }); facts = facts.filter(f => f.id !== id); deleting = ''; status = 'Memory deleted. You can restore it from Deleted memories.'; if (showDeleted) await refreshDeleted() }
    catch (e) { status = String(e) }
    finally { busy = false }
  }
</script>
<section class:page={part === 'memory'} aria-label={agentId ? "Agent memory" : part === 'profile' ? "Profile" : part === 'memory' ? "Saved memories" : "Profile and memory"}>
  {#if showProfile}
  <p class="settings-intro">{part === 'profile' ? 'Your profile guides replies.' : 'Your profile and saved facts guide replies.'}</p>
  <div class="profile-fields">
    <label>Name<input bind:value={profile.name} disabled={!loaded || busy} autocomplete="name" /></label>
    <label>Preferred name<input bind:value={profile.preferredName} disabled={!loaded || busy} autocomplete="nickname" /></label>
    <label>Work<input bind:value={profile.work} disabled={!loaded || busy} placeholder="What do you do?" /></label>
    <label>Instructions<textarea bind:value={profile.instructions} disabled={!loaded || busy} rows="5" placeholder="How should Muniment respond?"></textarea></label>
    {#if profile.extra}<label>Other profile details<textarea bind:value={profile.extra} disabled={!loaded || busy} rows="4"></textarea></label>{/if}
  </div>
  <p class="path">Saved in memory/profile.md</p>
  <div class="actions">
    <button disabled={!loaded || busy} onclick={saveProfile}>Save profile</button>
    <label class="import">Load Markdown<input type="file" accept=".md,.txt,text/plain,text/markdown" disabled={!loaded || busy} onchange={importProfile} /></label>
  </div>
  {:else if agentId}<p>Facts saved here belong to this agent. Its chat can create, correct, and remove them.</p>{/if}
  {#if showMemory}
  {#if part === 'all'}<h4>Saved memories</h4>{/if}
  <p class="settings-intro">Save one fact and its source. Muniment recalls the facts that fit each message.</p>
  {#if part === 'memory'}
    <div class="toolbar">
      {#if facts.length}<input aria-label="Search memories" placeholder="Search memories" bind:value={filter} />{:else}<span class="spacer"></span>{/if}
      <button type="button" disabled={!loaded || busy} onclick={() => { draft = { id: '', title: '', content: '', source: 'Added in settings' }; titleInput?.focus() }}>Add memory</button>
      <button type="button" disabled={!facts.length} onclick={exportMemories}>Export</button>
    </div>
    <section class="block" aria-labelledby="recall-heading">
      <h4 id="recall-heading" class="settings-heading">Recall</h4>
      <div class="rows">
        {#each RECALL as [key, label, help] (key)}
          <div class="row"><div><strong>{label}</strong><p>{help}</p></div><Toggle checked={recall[key]} {label} onchange={(on) => { recall = saveDevicePreference('muniment.memory-recall', { ...recall, [key]: on }) }} /></div>
        {/each}
      </div>
    </section>
    <h4 class="settings-heading">Saved memories <span class="count">{facts.length}</span></h4>
  {:else if facts.length}<input aria-label="Search memories" placeholder="Search memories" bind:value={filter} />{/if}
  {#each shown as fact (fact.id)}
    <article>
      <strong>{fact.title}</strong>
      <p class="fact">{fact.content}</p>
      <p class="source">{fact.source}</p>
      <div class="actions">
        <button disabled={busy} onclick={() => { draft = { ...fact }; deleting = '' }}>Edit</button>
        {#if deleting === fact.id}
          <button disabled={busy} onclick={() => removeFact(fact.id)}>Confirm delete</button>
          <button onclick={() => { deleting = '' }}>Cancel</button>
        {:else}<button disabled={busy} onclick={() => { deleting = fact.id }}>Delete</button>{/if}
      </div>
    </article>
  {:else}<p>{filter ? 'No matching memories.' : 'No saved memories yet.'}</p>{/each}
  <button aria-expanded={showDeleted} onclick={() => { showDeleted = !showDeleted; if (showDeleted) void refreshDeleted() }}>Deleted memories</button>
  {#if showDeleted}
    <p>Deleted memories stay out of recall. Restore one if it was removed by mistake.</p>
    {#each deletedFacts as fact (fact.id)}
      <article><strong>{fact.title}</strong><p class="fact">{fact.content}</p><p class="source">{fact.source}</p><div class="actions"><button disabled={busy} onclick={() => restoreFact(fact.id)}>Restore {fact.title}</button></div></article>
    {:else}<p>No deleted memories.</p>{/each}
  {/if}
  <form onsubmit={saveFact}>
    <h4 class="settings-heading">{draft.id ? 'Edit memory' : 'Add memory'}</h4>
    <label>Title<input bind:this={titleInput} bind:value={draft.title} maxlength="200" required disabled={!loaded || busy} /></label>
    <label>Fact<textarea bind:value={draft.content} rows="3" required disabled={!loaded || busy}></textarea></label>
    <label>Source<input bind:value={draft.source} required disabled={!loaded || busy} /></label>
    <div class="actions"><button disabled={!loaded || busy}>Save memory</button>
      {#if draft.id}<button type="button" onclick={() => { draft = { id: '', title: '', content: '', source: 'Added in settings' } }}>Cancel edit</button>{/if}
    </div>
  </form>
  {/if}
  {#if status}<p role="status">{status}</p>{/if}
</section>
<style>
  /* Settings → Memory: the intro, a toolbar, Recall, then the saved facts in one
     bordered list and the add form as a card. An agent's memory keeps its plain list. */
  .page { gap: 14px; max-width: 760px; }
  .page > .settings-heading { margin: 14px 0 0; }
  .toolbar { display: flex; align-items: center; gap: 8px; }
  .toolbar input { flex: 1; }
  .toolbar .spacer { flex: 1; }
  .toolbar button, .page form button { background: var(--faint); border-color: transparent; }
  .block { display: grid; gap: 10px; margin-top: 14px; }
  .block h4 { margin: 0; }
  .rows { display: grid; padding: 0 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 12px 0; }
  .row + .row { border-top: 1px solid var(--border); }
  .row > div { display: grid; gap: 2px; }
  .count { margin-left: 6px; color: var(--muted); font: var(--weight-regular) var(--text-12) var(--font-mono); }
  .page article { padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .page form { padding: 14px; border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .page form h4 { margin: 0; }
  .page > button[aria-expanded] { justify-self: start; border-color: transparent; background: transparent; color: var(--muted); }
  .page > button[aria-expanded]:hover { background: var(--faint); color: var(--ink); }
  section, form, .profile-fields { display: grid; gap: 10px; }
  p { margin: 0; color: var(--muted); font-size: var(--text-13); }
  label, strong { font-size: var(--text-13); }
  label { display: grid; gap: 5px; }
  input, textarea { box-sizing: border-box; width: 100%; padding: 9px 10px; font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--paper); border: 1px solid var(--border); border-radius: var(--radius-control); }
  textarea { resize: vertical; line-height: 1.5; }
  button, .import { font: inherit; font-size: var(--text-13); padding: 5px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); cursor: pointer; }
  button:hover, .import:hover { background: var(--faint); }
  button:disabled { opacity: .5; cursor: default; }
  .import { position: relative; overflow: hidden; }
  .import input { position: absolute; inset: 0; opacity: 0; cursor: pointer; }
  .import:focus-within { outline: 2px solid var(--ink); }
  .actions { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  h4 { margin: 16px 0 0; font-size: var(--text-13); }
  article { display: grid; gap: 6px; padding: 12px 0; border-bottom: 1px solid var(--border); }
  .fact { white-space: pre-wrap; color: var(--ink); }
  .source, .path { font-size: var(--text-12); overflow-wrap: anywhere; }
</style>
