<script>
  import { onMount, tick } from 'svelte'
  import { parseProfile, serializeProfile } from './profile-fields.js'
  import Toggle from './Toggle.svelte'
  import PageHead from './ui/PageHead.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import Button from './ui/Button.svelte'
  import SegmentedField from './ui/SegmentedField.svelte'
  import LucideIcon from './LucideIcon.svelte'
  import PopupClose from './PopupClose.svelte'
  import { dialogDismiss } from './dialog-dismiss.js'
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
    ['ask', 'Ask before saving', 'New memories, edits and deletes wait for your approval, in the chat or under Saved memories.'],
    ['use', 'Use memories in replies', 'Each chat starts with your most recalled facts and searches for the rest.'],
  ]
  const TRUST = {
    user: 'Added by you',
    conversation: 'Saved from a chat',
    external: 'Saved after web, file or tool content. Chats reach it only by search. Confirm or edit it to trust it.',
  }
  const TRUST_ICON = { user: 'user', conversation: 'messages-square' }
  // The list renders a page of rows at a time so hundreds of facts stay fast.
  const PAGE = 50
  const uid = $props.id()
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
  let trustFilter = $state('all')
  let limit = $state(PAGE)
  let deletedLimit = $state(PAGE)
  // One row opens at a time, keyed by list and fact id.
  let expanded = $state('')
  const unconfirmed = $derived(facts.filter(f => f.trust === 'external').length)
  const shown = $derived(facts.filter(f => (trustFilter === 'all' || f.trust === trustFilter) && `${f.title} ${f.content} ${f.source}`.toLowerCase().includes(filter.toLowerCase())))
  function toggle(key) { expanded = expanded === key ? '' : key; deleting = ''; if (draft.id) clearDraft() }
  function clearDraft() { draft = { id: '', title: '', content: '', source: 'Added in settings' } }
  async function editFact(fact) {
    draft = { ...fact }
    deleting = ''
    await tick()
    titleInput?.focus()
  }
  // Add memory opens a modal form and returns focus to its opener on close.
  let adding = $state(false)
  let addDialog = $state()
  let opener = null
  async function openAdd(event) {
    opener = event?.currentTarget ?? document.activeElement
    clearDraft()
    adding = true
    await tick()
    if (!addDialog.open) addDialog.showModal()
    titleInput?.focus()
  }
  function closeAdd() {
    if (addDialog?.open) addDialog.close()
    adding = false
    clearDraft()
    opener?.focus?.({ preventScroll: true })
  }
  // A new fact opens in the list, on a page the list shows.
  async function reveal(id) {
    const index = shown.findIndex(f => f.id === id)
    if (index < 0) return
    if (index >= limit) limit = (Math.floor(index / PAGE) + 1) * PAGE
    expanded = `saved:${id}`
    await tick()
    document.getElementById(`${uid}-saved:${id}`)?.scrollIntoView?.({ block: 'nearest' })
  }
  // Proposed changes from finished chats wait here until the user applies or
  // dismisses them. A runtime without the action leaves the list empty.
  const OP = { add: 'Add', update: 'Update', delete: 'Delete' }
  const when = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })
  let pending = $state([])
  let reviewing = $state(false)
  // A failed apply keeps its row and error until the user closes it, even
  // after the reload drops the proposal.
  let failed = $state(null)
  const proposals = $derived(failed && !pending.some(p => p.id === failed.proposal.id) ? [...pending.slice(0, failed.index), failed.proposal, ...pending.slice(failed.index)] : pending)
  const proposed = (p) => p.op === 'delete' ? p.before : p.after
  function suggested(value) {
    const date = new Date(value)
    return Number.isNaN(date.getTime()) ? value : when.format(date)
  }
  async function loadPending() {
    try { pending = await memory.invoke('memory_pending') || [] }
    catch { pending = [] }
    if (!proposals.length) reviewing = false
  }
  function closeFailed() {
    if (expanded === `pending:${failed?.proposal.id}`) expanded = ''
    failed = null
    if (!proposals.length) reviewing = false
  }
  async function applyPending(proposal) {
    busy = true
    failed = null
    try {
      const fact = await memory.invoke('memory_pending_apply', { id: proposal.id })
      if (proposal.op === 'delete') facts = facts.filter(f => f.id !== proposal.fact_id)
      else if (fact) facts = facts.some(f => f.id === fact.id) ? facts.map(f => f.id === fact.id ? fact : f) : [...facts, fact]
      if (proposal.op === 'delete' && showDeleted) await refreshDeleted()
      expanded = ''
      status = 'Suggested change applied.'
    } catch (e) {
      failed = { proposal, index: proposals.findIndex(p => p.id === proposal.id), error: String(e) }
      try { facts = await memory.invoke('memory_facts') } catch {}
    } finally {
      await loadPending()
      busy = false
    }
  }
  async function dismissPending(proposal) {
    if (failed?.proposal.id === proposal.id && !pending.some(p => p.id === proposal.id)) return closeFailed()
    busy = true
    try {
      await memory.invoke('memory_pending_dismiss', { id: proposal.id })
      if (failed?.proposal.id === proposal.id) failed = null
      expanded = ''
      status = 'Suggested change dismissed.'
    } catch (e) { status = String(e) }
    finally {
      await loadPending()
      busy = false
    }
  }
  onMount(async () => {
    void loadPending()
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
      const saved = await memory.invoke('memory_fact_save', { fact: { ...draft } })
      facts = await memory.invoke('memory_facts')
      clearDraft()
      status = 'Memory saved.'
      if (adding) {
        filter = ''
        trustFilter = 'all'
        closeAdd()
        if (saved?.id) await reveal(saved.id)
      }
    } catch (e) { status = String(e) }
    finally { busy = false }
  }
  let deleting = $state('')
  // An unconfirmed row acts from its icons: edit and delete open the row at
  // that step, and confirm saves the fact unchanged as the user's own.
  async function quickEdit(fact) {
    expanded = `saved:${fact.id}`
    await editFact(fact)
  }
  function quickDelete(fact) {
    if (draft.id) clearDraft()
    expanded = `saved:${fact.id}`
    deleting = fact.id
  }
  async function confirmFact(fact) {
    busy = true
    try {
      await memory.invoke('memory_fact_save', { fact: { ...fact } })
      facts = await memory.invoke('memory_facts')
      status = 'Memory confirmed.'
    } catch (e) { status = String(e) }
    finally { busy = false }
  }
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
        <Button variant="primary" disabled={!loaded || busy} onclick={saveProfile}>Save profile</Button>
      {/snippet}
    </PageHead>
    <Group heading="About you" info="Saved in memory/profile.md.">
      <Row title="Name"><input class="row-input" aria-label="Name" bind:value={profile.name} disabled={!loaded || busy} autocomplete="name" /></Row>
      <Row title="Preferred name"><input class="row-input" aria-label="Preferred name" bind:value={profile.preferredName} disabled={!loaded || busy} autocomplete="nickname" /></Row>
      <Row title="Work"><input class="row-input" aria-label="Work" bind:value={profile.work} disabled={!loaded || busy} placeholder="What do you do?" /></Row>
    </Group>
    <Group heading="Instructions">
      <div class="field"><textarea aria-label="Instructions" bind:value={profile.instructions} disabled={!loaded || busy} rows="5" placeholder="How should Muniment respond?"></textarea></div>
      {#if profile.extra}<label class="field">Other profile details<textarea bind:value={profile.extra} disabled={!loaded || busy} rows="4"></textarea></label>{/if}
    </Group>
  {:else if agentId}<p>Facts saved here belong to this agent. Its chat can create, correct, and remove them.</p>{/if}
  {#if showMemory}
    {#if !agentId}
      <PageHead intro="Save one fact and its source. Muniment recalls the facts that fit each message.">
        {#snippet actions()}
          <Button disabled={!facts.length} onclick={exportMemories}>Export</Button>
        {/snippet}
      </PageHead>
    {/if}
    {#if part === 'memory'}
      <Group heading="Recall">
        {#each RECALL as [key, label, help] (key)}
          <Row title={label} info={help}><Toggle checked={recall[key]} {label} disabled={!recallLoaded || recallSaving || (key === 'ask' && !recall.learn)} onchange={(on) => changeRecall(key, on)} /></Row>
        {/each}
      </Group>
      <MemoryImports {tauri} />
    {/if}
    <Group heading="Saved memories" count={facts.length} plain>
      {#snippet trail()}<Button icon="plus" iconOnly aria-label="Add memory" disabled={!loaded} onclick={openAdd} />{/snippet}
      {#snippet actions()}
        <button class="disclosure" aria-expanded={showDeleted} onclick={() => { showDeleted = !showDeleted; deletedLimit = PAGE; if (showDeleted) void refreshDeleted() }}>Deleted memories</button>
      {/snippet}
      {#if proposals.length}
        <div class="pending">
          <div class="pending-bar">
            <span class="pending-count">{proposals.length === 1 ? '1 suggested change' : `${proposals.length} suggested changes`}</span>
            <Button variant="outline" aria-expanded={reviewing} aria-controls={`${uid}-pending`} onclick={() => { reviewing = !reviewing }}>{reviewing ? 'Hide' : 'Review'}</Button>
          </div>
          {#if reviewing}
            <ul class="proposals" id={`${uid}-pending`} aria-label="Suggested changes">
              {#each proposals as proposal (proposal.id)}{@render proposalRow(proposal)}{/each}
            </ul>
          {/if}
        </div>
      {/if}
      {#if facts.length}
        <div class="toolbar">
          <input class="search" type="search" autocomplete="off" aria-label="Search memories" placeholder="Search memories" bind:value={filter} oninput={() => { limit = PAGE }} />
          {#if unconfirmed || trustFilter !== 'all'}
            <SegmentedField hideLabel label="Show memories" value={trustFilter} options={[{ value: 'all', label: 'All' }, { value: 'external', label: `Unconfirmed (${unconfirmed})` }]} onchange={(value) => { trustFilter = value; limit = PAGE }} />
          {/if}
        </div>
      {/if}
      {@render factList(shown, 'saved', limit, () => { limit += PAGE }, filter ? 'No matching memories.' : trustFilter === 'external' ? 'No unconfirmed memories.' : 'No saved memories yet.')}
    </Group>
    {#if showDeleted}
      <Group heading="Deleted memories" info="Deleted memories stay out of recall. Restore one if it was removed by mistake." plain>
        {@render factList(deletedFacts, 'deleted', deletedLimit, () => { deletedLimit += PAGE }, 'No deleted memories.')}
      </Group>
    {/if}
    <dialog class="memory-dialog" data-panel="memory-add" data-panel-variant="overlay" bind:this={addDialog} use:dialogDismiss={{ onclose: closeAdd, disabled: busy }} aria-labelledby={`${uid}-add`}>
      {#if adding}
        <div class="dialog-head"><h2 id={`${uid}-add`}>Add memory</h2><PopupClose label="Close" disabled={busy} onclick={closeAdd} /></div>
        {@render factForm('Cancel', closeAdd)}
      {/if}
    </dialog>
  {/if}
  {#if status}<p role="status">{status}</p>{/if}
</section>
{#snippet factList(items, list, max, more, empty)}
  {#if items.length}
    <ul class="facts" aria-label={list === 'saved' ? 'Saved memories' : 'Deleted memories'}>
      {#each items.slice(0, max) as fact (fact.id)}{@render factRow(fact, list)}{/each}
    </ul>
    {#if items.length > max}
      <div class="more">
        <span class="tally">{max} of {items.length}</span>
        <Button onclick={more}>Show {Math.min(PAGE, items.length - max)} more</Button>
      </div>
    {/if}
  {:else}<p class="empty">{empty}</p>{/if}
{/snippet}
{#snippet factRow(fact, list)}
  {@const key = `${list}:${fact.id}`}
  {@const open = expanded === key}
  <li class="fact">
    <button type="button" class="fact-head" aria-expanded={open} aria-controls={`${uid}-${key}`} onclick={() => toggle(key)}>
      <span class="chevron" class:open><LucideIcon name="chevron-right" size={14} variant="action" /></span>
      <span class="fact-title">{fact.title}</span>
      <span class="fact-preview" aria-hidden="true">{fact.content}</span>
      {#if fact.trust === 'external' && list === 'saved'}<span class="quick-space"><span class="visually-hidden">Unconfirmed</span></span>
      {:else if fact.trust === 'external'}<span class="badge">Unconfirmed</span>
      {:else if TRUST_ICON[fact.trust]}<span class="trust-icon"><LucideIcon name={TRUST_ICON[fact.trust]} size={14} variant="action" /><span class="visually-hidden">{TRUST[fact.trust]}</span></span>{/if}
    </button>
    {#if fact.trust === 'external' && list === 'saved'}
      <span class="quick">
        <Button icon="pencil" iconOnly aria-label="Edit memory" disabled={busy} onclick={() => quickEdit(fact)} />
        <span class="danger"><Button icon="trash-2" iconOnly aria-label="Delete memory" disabled={busy} onclick={() => quickDelete(fact)} /></span>
        <span class="confirm"><Button icon="check" iconOnly aria-label="Confirm memory" disabled={busy} onclick={() => confirmFact(fact)} /></span>
      </span>
    {/if}
    {#if open}
      <div class="fact-detail" id={`${uid}-${key}`}>
        {#if list === 'saved' && draft.id === fact.id}<section aria-label="Edit memory">{@render factForm('Cancel edit', clearDraft)}</section>
        {:else}
          <p class="fact-content">{fact.content}</p>
          <dl class="fact-meta">
            <dt>Source</dt><dd class="record">{fact.source}</dd>
            {#if TRUST[fact.trust]}<dt>Trust</dt><dd>{TRUST[fact.trust]}</dd>{/if}
          </dl>
          <div class="fact-actions">
            {#if list === 'deleted'}<Button disabled={busy} aria-label={`Restore ${fact.title}`} onclick={() => restoreFact(fact.id)}>Restore</Button>
            {:else if deleting === fact.id}
              <Button disabled={busy} onclick={() => removeFact(fact.id)}>Confirm delete</Button>
              <Button onclick={() => { deleting = '' }}>Cancel</Button>
            {:else if fact.trust !== 'external'}
              <Button disabled={busy} onclick={() => editFact(fact)}>Edit</Button>
              <Button disabled={busy} onclick={() => { deleting = fact.id }}>Delete</Button>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </li>
{/snippet}
{#snippet proposalRow(proposal)}
  {@const key = `pending:${proposal.id}`}
  {@const open = expanded === key}
  {@const fact = proposed(proposal)}
  {@const error = failed?.proposal.id === proposal.id ? failed.error : ''}
  <li class="fact">
    <button type="button" class="fact-head proposal-head" aria-expanded={open} aria-controls={`${uid}-${key}`} onclick={() => toggle(key)}>
      <span class="chevron" class:open><LucideIcon name="chevron-right" size={14} variant="action" /></span>
      <span class="op">{OP[proposal.op]}</span>
      <span class="fact-title">{fact?.title}</span>
      <span class="fact-preview" aria-hidden="true">{fact?.content}</span>
    </button>
    {#if open}
      <div class="fact-detail" id={`${uid}-${key}`}>
        {#if proposal.op === 'update'}
          {#each [['Before', proposal.before], ['After', proposal.after]] as [label, side] (label)}
            <div class="change">
              <p class="change-label">{label}</p>
              {#if proposal.before?.title !== proposal.after?.title}<p class="change-title">{side?.title}</p>{/if}
              <p class="fact-content">{side?.content}</p>
            </div>
          {/each}
        {:else}
          <div class="change">
            <p class="change-label">{proposal.op === 'delete' ? 'Removes' : 'Adds'}</p>
            <p class="fact-content">{fact?.content}</p>
          </div>
        {/if}
        <dl class="fact-meta">
          <dt>Source</dt><dd class="record">{proposal.source}</dd>
          <dt>Suggested</dt><dd>{suggested(proposal.created)}</dd>
        </dl>
        {#if error}<p class="error" role="alert">{error}</p>{/if}
        <div class="fact-actions">
          {#if !error}<Button disabled={busy} onclick={() => applyPending(proposal)}>Apply</Button>{/if}
          <Button disabled={busy} onclick={() => dismissPending(proposal)}>Dismiss</Button>
        </div>
      </div>
    {/if}
  </li>
{/snippet}
{#snippet factForm(cancelLabel, cancel)}
  <form class="fact-form" onsubmit={saveFact}>
    <label class="field">Title<input bind:this={titleInput} bind:value={draft.title} maxlength="200" required disabled={!loaded || busy} /></label>
    <label class="field">Fact<textarea bind:value={draft.content} rows="3" required disabled={!loaded || busy}></textarea></label>
    <label class="field">Source<input bind:value={draft.source} required disabled={!loaded || busy} /></label>
    <div class="field-actions">
      <Button onclick={cancel}>{cancelLabel}</Button>
      <Button type="submit" disabled={!loaded || busy}>Save memory</Button>
    </div>
  </form>
{/snippet}
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
  .search { flex: 1 1 200px; min-width: 0; }
  textarea { resize: vertical; line-height: 1.5; }
  .import { position: relative; overflow: hidden; display: inline-flex; align-items: center; min-height: 32px; box-sizing: border-box; padding: 6px 10px; border-radius: var(--radius-control); background: var(--faint); color: var(--ink); font: var(--text-13) var(--font-human); cursor: pointer; }
  .import:hover, .import:has(input:focus-visible) { background: var(--border); }
  .import input { position: absolute; inset: 0; opacity: 0; cursor: pointer; }
  .disclosure { min-height: 32px; padding: 6px 10px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); font: var(--text-13) var(--font-human); cursor: pointer; }
  .disclosure:hover { background: var(--faint); color: var(--ink); }
  .disclosure { white-space: nowrap; }
  .disclosure[aria-expanded="true"] { color: var(--ink); }
  .toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-width: 0; }
  .empty { padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); }
  /* One 36px row per fact: title, a one-line preview and a trust mark. The
     row opens in place to show the whole fact and its actions. */
  .facts { container-type: inline-size; display: grid; margin: 0; padding: 0; list-style: none; border: 1px solid var(--border); border-radius: var(--radius-panel); overflow: hidden; }
  .fact { position: relative; min-width: 0; }
  .fact + .fact::before { content: ''; position: absolute; top: 0; left: 12px; right: 12px; border-top: 1px solid var(--border); }
  .fact-head { display: grid; grid-template-columns: 14px minmax(0, clamp(96px, 30%, 240px)) minmax(0, 1fr) auto; align-items: center; gap: 10px; width: 100%; min-height: 36px; box-sizing: border-box; padding: 8px 12px; border: 0; background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); line-height: 20px; text-align: left; cursor: pointer; }
  .fact-head:hover, .fact-head:focus-visible { background: var(--faint); }
  .chevron { display: inline-flex; color: var(--muted); }
  .chevron.open { transform: rotate(90deg); }
  .fact-title, .fact-preview { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .fact-title { font-weight: var(--weight-semibold); }
  .fact-preview { color: var(--muted); }
  .fact-head[aria-expanded="true"] .fact-preview { visibility: hidden; }
  .badge { padding: 0 6px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--ink); font-size: var(--text-12); line-height: 18px; white-space: nowrap; }
  .trust-icon { display: inline-flex; color: var(--muted); }
  /* An unconfirmed row holds its three icon buttons over a reserved slot at
     the head's right end, outside the head button so a click acts once. */
  .quick-space { width: 80px; height: 20px; }
  .quick { position: absolute; top: 6px; right: 12px; display: flex; gap: 4px; }
  .quick :global(button) { min-width: 24px; min-height: 24px; padding: 3px; color: var(--ink); }
  /* Each icon's border takes the icon's own color on hover and focus. */
  .quick :global(button:hover:not(:disabled)), .quick :global(button:focus-visible) { border-color: currentColor; }
  .quick .danger :global(button) { color: var(--oxide); }
  /* Confirm takes the theme's accent, else its signal. Moss keeps its green
     signal even in the vibrant set, whose accent is not green. */
  .quick .confirm { --confirm: var(--accent, var(--signal)); }
  :global(:root[data-theme^="moss-vibrant-"]) .quick .confirm { --confirm: var(--signal); }
  .quick .confirm :global(button) { color: var(--confirm); }
  .quick > span { display: inline-flex; }
  .fact-detail { display: grid; gap: 10px; padding: 2px 12px 12px 36px; }
  .fact-content { white-space: pre-wrap; overflow-wrap: anywhere; color: var(--ink); }
  .fact-meta { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 4px 12px; margin: 0; font-size: var(--text-12); }
  .fact-meta dt { color: var(--muted); }
  .fact-meta dd { margin: 0; color: var(--ink); overflow-wrap: anywhere; }
  .record { font-family: var(--font-mono); }
  .fact-actions { display: flex; flex-wrap: wrap; gap: 8px; }
  .fact-actions:empty { display: none; }
  .fact-form { display: grid; gap: 12px; }
  .fact-form .field { padding: 0; }
  .fact-form .field-actions { padding: 0; }
  .memory-dialog { position: fixed; inset: 0; margin: auto; width: min(520px, calc(100vw - 48px)); height: fit-content; max-height: calc(100vh - 48px); overflow: auto; box-sizing: border-box; padding: 20px 24px 24px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--paper); color: var(--ink); font-family: var(--font-human); }
  .memory-dialog::backdrop { background: var(--overlay-backdrop); }
  .memory-dialog[open] { display: grid; gap: 16px; }
  .dialog-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .dialog-head h2 { margin: 0; font-size: var(--text-17); font-weight: var(--weight-semibold); }
  /* Suggested changes sit in one card above the search: a count bar, then
     one row per proposal in the fact row grid with its operation first. */
  .pending { container-type: inline-size; display: grid; border: 1px solid var(--border); border-radius: var(--radius-panel); overflow: hidden; }
  .pending-bar { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 36px; padding: 4px 4px 4px 12px; background: var(--faint); }
  .pending-count { color: var(--ink); font-size: var(--text-13); }
  .proposals { display: grid; margin: 0; padding: 0; list-style: none; border-top: 1px solid var(--border); }
  .proposal-head { grid-template-columns: 14px 52px minmax(0, clamp(96px, 30%, 240px)) minmax(0, 1fr); }
  .op { justify-self: start; padding: 0 6px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--ink); font-size: var(--text-12); line-height: 18px; }
  .change { display: grid; gap: 2px; }
  .change-label { color: var(--muted); font-size: var(--text-12); }
  .change-title { color: var(--ink); font-weight: var(--weight-semibold); }
  .error { color: var(--ink); }
  .more { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .tally { color: var(--muted); font: var(--text-12) var(--font-mono); }
  /* A narrow list puts the preview under the title. */
  @container (max-width: 440px) {
    .fact-head { grid-template-columns: 14px minmax(0, 1fr) auto; row-gap: 0; }
    .fact-preview { grid-area: 2 / 2 / 3 / 4; }
    .fact-head[aria-expanded="true"] .fact-preview { display: none; }
    .proposal-head { grid-template-columns: 14px 52px minmax(0, 1fr); }
    .proposal-head .fact-preview { grid-area: 2 / 3 / 3 / 4; }
  }
</style>
