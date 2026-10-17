<script>
  // The deny, ask-first and always-allow rule lists, with a form that adds
  // one exact rule. An agent's profile and Settings → Permissions share it.
  // `onsave(list, requests)` saves one list in full.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { PATH_OPERATIONS, RULE_KINDS, RULE_LISTS, SHELLS, emptyRuleForm, ruleFromForm, ruleLabel } from './permission-rules.js'

  let { rules = {}, busy = false, onsave } = $props()
  let form = $state(null)
  let error = $state('')

  function add(event) {
    event.preventDefault()
    let request
    try { request = ruleFromForm(form) } catch (failure) { error = failure.message; return }
    const list = form.list
    form = null
    error = ''
    void onsave(list, [...(rules[list] ?? []), request])
  }
</script>

{#each RULE_LISTS as [list, heading] (list)}
  <h4>{heading}</h4>
  <ul class="rules" aria-label={heading}>
    {#each rules[list] ?? [] as request, index (index)}<li><span class="record">{ruleLabel(request)}</span><button class="quiet icon" aria-label={`Remove ${heading.toLowerCase()} rule ${index + 1}`} disabled={busy} onclick={() => onsave(list, rules[list].filter((_, at) => at !== index))}><LucideIcon name="x" size={14} /></button></li>{:else}<li class="muted">None</li>{/each}
  </ul>
{/each}
{#if form}
  <form class="rule-form" aria-label="New permission rule" onsubmit={add}>
    <label>List<select bind:value={form.list}>{#each RULE_LISTS as [list, heading] (list)}<option value={list}>{heading}</option>{/each}</select></label>
    <label>Kind<select bind:value={form.kind}>{#each RULE_KINDS as [kind, label] (kind)}<option value={kind}>{label}</option>{/each}</select></label>
    {#if form.kind === 'command'}
      <label>Shell<select bind:value={form.shell}>{#each SHELLS as shell (shell)}<option value={shell}>{shell}</option>{/each}</select></label>
      <label>Command<input class="record" bind:value={form.command} spellcheck="false" /></label>
      <label>Folder<input class="record" bind:value={form.directory} placeholder="/full/path" spellcheck="false" /></label>
    {:else if form.kind === 'path'}
      <label>Action<select bind:value={form.operation}>{#each PATH_OPERATIONS as operation (operation)}<option value={operation}>{operation}</option>{/each}</select></label>
      <label>Path<input class="record" bind:value={form.path} placeholder="/full/path" spellcheck="false" /></label>
    {:else}
      <label>Tool<input class="record" bind:value={form.name} placeholder="mcp__server__tool" spellcheck="false" /></label>
      <label>Input<textarea class="record" rows="2" bind:value={form.input} spellcheck="false"></textarea></label>
    {/if}
    <small>Rules match exactly. Deny wins over Ask first, and Ask first over Always allow.</small>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="actions"><button disabled={busy}>Add rule</button><button type="button" onclick={() => { form = null; error = '' }}>Cancel</button></div>
  </form>
{:else}<div class="actions"><button disabled={busy} onclick={() => { form = { ...emptyRuleForm(), list: 'always_allow' } }}>Add rule</button></div>{/if}

<style>
  h4 { margin: 6px 0 0; font-size: var(--text-12); font-weight: 600; }
  ul { display: grid; gap: 4px; margin: 0; padding: 0; list-style: none; }
  .rules li { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-width: 0; font-size: var(--text-13); }
  .record { min-width: 0; overflow-wrap: anywhere; font-family: var(--font-mono); font-size: var(--text-12); }
  .muted { color: var(--muted); font-size: var(--text-12); }
  form, label { display: grid; gap: 8px; font-size: var(--text-12); }
  .rule-form { padding: 8px; border: 1px solid var(--border); border-radius: var(--radius-control); }
  input, textarea, select { box-sizing: border-box; width: 100%; min-width: 0; padding: 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; }
  textarea { resize: vertical; line-height: 1.5; }
  small { color: var(--muted); line-height: 1.5; }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; }
  button { font: inherit; font-size: var(--text-12); color: var(--ink); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 8px; background: transparent; cursor: pointer; }
  button:hover { background: var(--faint); }
  button:disabled { opacity: .5; cursor: default; }
  .quiet { border-color: transparent; padding: 4px; line-height: 0; }
  .icon { display: grid; place-items: center; min-width: 24px; min-height: 24px; }
  .error { margin: 0; color: var(--oxide); font-size: var(--text-12); }
</style>
