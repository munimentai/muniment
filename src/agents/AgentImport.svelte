<script>
  // Imports an agent from a share link, a template file or pasted text. Core
  // reads the source and answers a preview: the profile, the routines it
  // proposes, the skills it matched or proposes, the tools to set up and
  // notes on routines it could not map. Import saves the preview.
  import AgentDialog from './AgentDialog.svelte'
  import Button from '../lib/ui/Button.svelte'
  import AgentAvatar from '../lib/AgentAvatar.svelte'
  import { commitImport, importPreview, importSource, previewFacts, readTemplateFile } from './import.js'
  import { triggerSummary } from './routine-labels.js'
  import { LOCAL_RUNTIME_ID } from '../runtimes/runtimes.js'

  let { tauri, runtimes = [], onclose, onimported = () => {} } = $props()
  let way = $state('link')
  let link = $state('')
  let paste = $state('')
  let fileText = $state('')
  let fileName = $state('')
  let runtime = $state(LOCAL_RUNTIME_ID)
  let preview = $state(null)
  let keepMemories = $state(true)
  let busy = $state(false)
  let error = $state('')
  const facts = $derived(preview ? previewFacts(preview) : null)

  async function chooseFile(event) {
    const file = event.currentTarget.files?.[0]
    error = ''
    try { fileText = await readTemplateFile(file); fileName = file?.name ?? '' } catch (failure) { error = String(failure?.message ?? failure); fileText = ''; fileName = '' }
  }

  async function read() {
    busy = true
    error = ''
    try { preview = await importPreview(tauri, runtime, importSource({ way, link, file: fileText, paste })) } catch (failure) { error = String(failure?.message ?? failure) } finally { busy = false }
  }

  async function commit() {
    busy = true
    error = ''
    try {
      const answer = await commitImport(tauri, runtime, preview, { memories: keepMemories })
      onimported({ ...answer.agent, runtimeId: runtime })
      onclose()
    } catch (failure) {
      error = String(failure?.message ?? failure)
    } finally {
      busy = false
    }
  }
</script>

<AgentDialog title="Import agent" panel="import" {busy} width={600} {onclose}>
  {#if !preview}
    <p class="intro">Bring in an agent from a share link, a template file or pasted text. Nothing saves until you import.</p>
    <div class="ways" role="radiogroup" aria-label="Import from">
      {#each [['link', 'Share link'], ['file', 'File'], ['paste', 'Paste']] as [value, label] (value)}
        <label class="way"><input type="radio" name="import-way" {value} bind:group={way} disabled={busy} />{label}</label>
      {/each}
    </div>
    {#if way === 'link'}
      <label>Share link<input class="mono" bind:value={link} placeholder="https://" spellcheck="false" disabled={busy} /></label>
      <p class="note">A public share page holds the profile only.</p>
    {:else if way === 'file'}
      <label>Template file<input type="file" accept=".json,.md,.markdown,.txt" onchange={chooseFile} disabled={busy} /></label>
      {#if fileName}<p class="note mono">{fileName}</p>{/if}
    {:else}
      <label>Template<textarea class="mono" rows="8" bind:value={paste} placeholder="JSON, or Markdown with a name heading" spellcheck="false" disabled={busy}></textarea></label>
    {/if}
    {#if runtimes.length > 1}
      <label>Machine<select bind:value={runtime} disabled={busy}>{#each runtimes as entry (entry.id)}<option value={entry.id}>{entry.name}</option>{/each}</select></label>
    {/if}
  {:else}
    <div class="profile">
      <AgentAvatar agent={{ ...preview.agent, id: preview.agent.id || preview.agent.name }} size={40} />
      <div><strong>{preview.agent.name}</strong>{#if preview.agent.label}<span class="muted">{preview.agent.label}</span>{/if}<span class="record">{facts.source}</span></div>
    </div>
    {#if facts.coverage}<p class="warning" role="note">{facts.coverage}</p>{/if}
    {#if preview.agent.instructions}<p class="instructions">{preview.agent.instructions}</p>{/if}
    <section aria-label="Routines">
      <h3>Routines</h3>
      <p class="note">{facts.routines}</p>
      <ul>{#each preview.agent.routines ?? [] as routine (routine.id)}<li><strong>{routine.name}</strong><span class="record">{triggerSummary(routine.trigger)}</span></li>{/each}</ul>
      {#each preview.routine_notes ?? [] as note (note.name)}<p class="note"><strong>{note.name}:</strong> {note.note}</p>{/each}
    </section>
    <section aria-label="Skills">
      <h3>Skills</h3>
      {#each preview.skills?.matched ?? [] as skill (skill.id)}<p class="item">{skill.name}<span class="record">installed</span></p>{/each}
      {#each preview.skills?.proposals ?? [] as skill (skill.name)}<p class="item">{skill.name}<span class="record">kept as reference text</span></p>{/each}
      {#if !preview.skills?.matched?.length && !preview.skills?.proposals?.length}<p class="note">The template names no skills.</p>{/if}
    </section>
    <section aria-label="Setup">
      <h3>Tools</h3>
      {#each preview.tools?.matched ?? [] as tool (tool.name)}<p class="item">{tool.name}<span class="record">installed</span></p>{/each}
      {#each preview.tools?.setup ?? [] as tool (tool.name)}<p class="item">{tool.name}<span class="record">set up in Tools</span></p>{/each}
      {#if !preview.tools?.matched?.length && !preview.tools?.setup?.length}<p class="note">The template names no tools.</p>{/if}
    </section>
    {#if preview.memories?.length}
      <label class="check"><input type="checkbox" bind:checked={keepMemories} disabled={busy} />Import {preview.memories.length} {preview.memories.length === 1 ? 'memory' : 'memories'} as outside facts</label>
    {/if}
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#snippet actions()}
    {#if preview}<Button disabled={busy} onclick={() => { preview = null }}>Back</Button>{/if}
    <span class="spacer"></span>
    <Button disabled={busy} onclick={onclose}>Cancel</Button>
    {#if preview}
      <Button variant="primary" disabled={busy} onclick={commit}>{busy ? 'Importing…' : 'Import agent'}</Button>
    {:else}
      <Button variant="primary" disabled={busy} onclick={read}>{busy ? 'Reading…' : 'Preview'}</Button>
    {/if}
  {/snippet}
</AgentDialog>

<style>
  .ways { display: flex; flex-wrap: wrap; gap: 16px; }
  .way { display: flex; align-items: center; gap: 6px; color: var(--ink); font-size: var(--text-13); }
  label { display: grid; gap: 4px; color: var(--muted); font-size: var(--text-12); }
  input:not([type="radio"]):not([type="checkbox"]), select, textarea { box-sizing: border-box; width: 100%; min-height: 32px; padding: 5px 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; font-size: var(--text-13); }
  textarea { resize: vertical; line-height: 1.5; }
  .mono, .record { font-family: var(--font-mono); font-size: var(--text-12); }
  .record { color: var(--muted); }
  .intro { margin: 0; color: var(--muted); font-size: var(--text-13); line-height: 1.5; }
  .profile { display: flex; align-items: center; gap: 12px; padding-bottom: 12px; border-bottom: 1px solid var(--border); }
  .profile div { display: grid; gap: 2px; min-width: 0; }
  .muted { color: var(--muted); font-size: var(--text-13); }
  .instructions { margin: 0; max-height: 120px; overflow-y: auto; white-space: pre-wrap; font-size: var(--text-13); line-height: 1.5; }
  section { display: grid; gap: 4px; padding-bottom: 8px; border-bottom: 1px solid var(--border); }
  h3 { margin: 4px 0 0; color: var(--muted); font-size: var(--text-12); font-weight: 500; }
  ul { display: grid; gap: 4px; margin: 0; padding: 0; list-style: none; }
  li, .item { display: flex; flex-wrap: wrap; align-items: baseline; gap: 8px; margin: 0; font-size: var(--text-13); }
  .note { margin: 0; color: var(--muted); font-size: var(--text-12); line-height: 1.5; }
  .warning { margin: 0; padding: 8px 10px; border: 1px solid var(--warning); border-radius: var(--radius-control); font-size: var(--text-12); line-height: 1.5; }
  .check { display: flex; align-items: center; gap: 8px; color: var(--ink); font-size: var(--text-13); }
  .error { margin: 0; color: var(--danger); font-size: var(--text-12); }
  .spacer { flex: 1; }
</style>
