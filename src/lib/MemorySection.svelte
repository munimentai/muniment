<script>
  import { onMount } from 'svelte'
  import { parseProfile, serializeProfile } from './profile-fields.js'
  import Toggle from './Toggle.svelte'
  import PageHead from './ui/PageHead.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import Button from './ui/Button.svelte'
  import MemoryImports from './MemoryImports.svelte'
  import { RECALL_DEFAULTS, loadRecall, saveRecall } from './memory-recall.js'
  // `part` shows one Settings page: profile, memory, or both for an agent.
  let { tauri, agentId = null, part = 'all' } = $props()
  const showProfile = $derived(!agentId && part !== 'memory')
  const showMemory = $derived(part !== 'profile')
  // New chats read these choices from the profile.
  let recall = $state({ ...RECALL_DEFAULTS })
  let recallLoaded = $state(false)
  let recallSaving = $state(false)
  async function changeRecall(key, on) {
    const previous = recall
    const next = { ...recall, [key]: on }
    recall = next
    recallSaving = true
    try {
      recall = await saveRecall(tauri, next)
      status = 'Recall saved. New replies use it.'
    } catch (e) {
      recall = previous
      status = String(e)
    } finally { recallSaving = false }
  }
  const RECALL = [
    ['learn', 'Save facts from chats', 'A chat proposes a memory when it states something lasting about you or your work.'],
    ['ask', 'Ask before saving', 'Proposed memories wait for your approval in the chat.'],
    ['use', 'Use memories in replies', 'Each chat starts with your most recalled facts and searches for the rest.'],
  ]
  const TRUST = {
    user: 'Added by you',
    conversation: 'Saved from a chat',
    external: 'Saved after web, file or tool content. Chats reach it only by search. Edit it to confirm it.',
  }
  let titleInput = $state()
  function exportMemories() {
    const text = facts.map((fact) => `## ${fact.title}\n\n${fact.content}\n\nSource: ${fact.source}\nTrust: ${fact.trust}\n`).join('\n')
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
    if (part === 'memory' && !agentId) {
      loadRecall(tauri).then((value) => { recall = value; recallLoaded = true }).catch((e) => { status = String(e) })
    }
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
<section class="memory" aria-label={agentId ? "Agent memory" : part === 'profile' ? "Profile" : part === 'memory' ? "Saved memories" : "Profile and memory"}>
  {#if showProfile}
    <PageHead intro={part === 'profile' ? 'Your profile guides replies.' : 'Your profile and saved facts guide replies.'}>
      {#snippet actions()}
        <label class="import">Load Markdown<input type="file" accept=".md,.txt,text/plain,text/markdown" disabled={!loaded || busy} onchange={importProfile} /></label>
        <Button disabled={!loaded || busy} onclick={saveProfile}>Save profile</Button>
      {/snippet}
    </PageHead>
    <Group heading="About you" description="Saved in memory/profile.md.">
      <Row title="Name"><input class="row-input" aria-label="Name" bind:value={profile.name} disabled={!loaded || busy} autocomplete="name" /></Row>
      <Row title="Preferred name" description="What replies call you."><input class="row-input" aria-label="Preferred name" bind:value={profile.preferredName} disabled={!loaded || busy} autocomplete="nickname" /></Row>
      <Row title="Work"><input class="row-input" aria-label="Work" bind:value={profile.work} disabled={!loaded || busy} placeholder="What do you do?" /></Row>
    </Group>
    <Group heading="Instructions" description="How Muniment should respond to you.">
      <div class="field"><textarea aria-label="Instructions" bind:value={profile.instructions} disabled={!loaded || busy} rows="5" placeholder="How should Muniment respond?"></textarea></div>
      {#if profile.extra}<label class="field">Other profile details<textarea bind:value={profile.extra} disabled={!loaded || busy} rows="4"></textarea></label>{/if}
    </Group>
  {:else if agentId}<p>Facts saved here belong to this agent. Its chat can create, correct, and remove them.</p>{/if}
  {#if showMemory}
    {#if !agentId}
      <PageHead intro="Save one fact and its source. Muniment recalls the facts that fit each message.">
        {#snippet actions()}
          <Button disabled={!loaded || busy} onclick={() => { draft = { id: '', title: '', content: '', source: 'Added in settings' }; titleInput?.focus() }}>Add memory</Button>
          <Button disabled={!facts.length} onclick={exportMemories}>Export</Button>
        {/snippet}
      </PageHead>
    {/if}
    {#if part === 'memory'}
      <Group heading="Recall">
        {#each RECALL as [key, label, help] (key)}
          <Row title={label} description={help}><Toggle checked={recall[key]} {label} disabled={!recallLoaded || recallSaving || (key === 'ask' && !recall.learn)} onchange={(on) => changeRecall(key, on)} /></Row>
        {/each}
      </Group>
      <MemoryImports {tauri} />
    {/if}
    <Group heading="Saved memories" count={facts.length}>
      {#snippet actions()}
        {#if facts.length}<input class="search" type="search" autocomplete="off" aria-label="Search memories" placeholder="Search memories" bind:value={filter} />{/if}
        <button class="disclosure" aria-expanded={showDeleted} onclick={() => { showDeleted = !showDeleted; if (showDeleted) void refreshDeleted() }}>Deleted memories</button>
      {/snippet}
      {#each shown as fact (fact.id)}
        <Row title={fact.title}>
          {#snippet detail()}<p class="fact">{fact.content}</p><p class="source">{fact.source}</p>{#if TRUST[fact.trust]}<p class="trust" class:external={fact.trust === 'external'}>{TRUST[fact.trust]}</p>{/if}{/snippet}
          {#if deleting === fact.id}
            <Button disabled={busy} onclick={() => removeFact(fact.id)}>Confirm delete</Button>
            <Button onclick={() => { deleting = '' }}>Cancel</Button>
          {:else}
            <Button disabled={busy} onclick={() => { draft = { ...fact }; deleting = ''; titleInput?.focus() }}>Edit</Button>
            <Button disabled={busy} onclick={() => { deleting = fact.id }}>Delete</Button>
          {/if}
        </Row>
      {:else}<p>{filter ? 'No matching memories.' : 'No saved memories yet.'}</p>{/each}
    </Group>
    {#if showDeleted}
      <Group heading="Deleted memories" description="Deleted memories stay out of recall. Restore one if it was removed by mistake.">
        {#each deletedFacts as fact (fact.id)}
          <Row title={fact.title}>
            {#snippet detail()}<p class="fact">{fact.content}</p><p class="source">{fact.source}</p>{#if TRUST[fact.trust]}<p class="trust" class:external={fact.trust === 'external'}>{TRUST[fact.trust]}</p>{/if}{/snippet}
            <Button disabled={busy} onclick={() => restoreFact(fact.id)}>Restore {fact.title}</Button>
          </Row>
        {:else}<p>No deleted memories.</p>{/each}
      </Group>
    {/if}
    <form onsubmit={saveFact}>
      <Group heading={draft.id ? 'Edit memory' : 'Add memory'}>
        <label class="field">Title<input bind:this={titleInput} bind:value={draft.title} maxlength="200" required disabled={!loaded || busy} /></label>
        <label class="field">Fact<textarea bind:value={draft.content} rows="3" required disabled={!loaded || busy}></textarea></label>
        <label class="field">Source<input bind:value={draft.source} required disabled={!loaded || busy} /></label>
        <div class="field-actions">
          {#if draft.id}<Button onclick={() => { draft = { id: '', title: '', content: '', source: 'Added in settings' } }}>Cancel edit</Button>{/if}
          <Button type="submit" disabled={!loaded || busy}>Save memory</Button>
        </div>
      </Group>
    </form>
  {/if}
  {#if status}<p role="status">{status}</p>{/if}
</section>
<style>
  /* Profile and Memory build from PageHead, Group and Row like every Settings
     page. Form fields sit in their Group card, one per row. */
  .memory { display: grid; gap: var(--page-gap, 28px); }
  p { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .field { display: grid; gap: 6px; padding: 12px 0; color: var(--muted); font-size: var(--text-13); }
  .field-actions { display: flex; justify-content: flex-end; gap: 8px; padding: 10px 0; }
  input:not([type="file"]), textarea { box-sizing: border-box; width: 100%; min-height: 32px; padding: 7px 10px; font: var(--text-13) var(--font-human); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); }
  input:focus, textarea:focus { outline: none; border-color: var(--muted); }
  .row-input { width: 280px; max-width: 100%; }
  .search { width: 220px; }
  textarea { resize: vertical; line-height: 1.5; }
  .import { position: relative; overflow: hidden; display: inline-flex; align-items: center; min-height: 32px; box-sizing: border-box; padding: 6px 10px; border-radius: var(--radius-control); background: var(--faint); color: var(--ink); font: var(--text-13) var(--font-human); cursor: pointer; }
  .import:hover { background: var(--border); }
  .import input { position: absolute; inset: 0; opacity: 0; cursor: pointer; }
  .import:focus-within { outline: 2px solid var(--ink); }
  .disclosure { min-height: 32px; padding: 6px 10px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); font: var(--text-13) var(--font-human); cursor: pointer; }
  .disclosure:hover { background: var(--faint); color: var(--ink); }
  .fact { white-space: pre-wrap; color: var(--ink); }
  .source, .trust { font-size: var(--text-12); overflow-wrap: anywhere; }
  .trust.external { color: var(--ink); }
</style>
