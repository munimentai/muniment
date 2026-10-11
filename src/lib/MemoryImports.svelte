<script>
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import Button from './ui/Button.svelte'
  import { scanRows } from './onboarding-scan.js'

  // Memory that other assistants keep on this computer. The scan reads file
  // names, never bodies, and runs only when asked, because some roots sit
  // under Documents and a launch must not raise a folder prompt.
  let { tauri } = $props()
  let report = $state(null)
  let scanning = $state(false)
  let error = $state('')
  const rows = $derived(scanRows(report).filter((row) => row.fileCount > 0))
  const home = (path) => path?.replace(/^\/(Users|home)\/[^/]+/, '~').replace(/^[A-Z]:\\Users\\[^\\]+/, '~')

  async function scan() {
    scanning = true
    error = ''
    try {
      report = await tauri.invoke('onboarding_scan')
    } catch (_) {
      error = 'Muniment could not look for assistant memory. Try again.'
    } finally {
      scanning = false
    }
  }
</script>

<Group heading="Imports" description="Memory that other assistants keep on this computer.">
  {#snippet actions()}<Button disabled={scanning} onclick={scan}>{report ? 'Look again' : 'Look for memory'}</Button>{/snippet}
  {#if scanning}
    <p role="status">Looking for assistant memory…</p>
  {:else if error}
    <p role="alert">{error}</p>
  {:else if !report}
    <p>Claude Code, Codex, Obsidian and other assistants keep notes you can bring in.</p>
  {:else}
    {#each rows as row (row.assistantId)}
      <Row title={row.displayName} meta={row.fileCount === 1 ? '1 file' : `${row.fileCount} files`} description={row.capped ? 'The count may be incomplete.' : ''}>
        {#snippet detail()}<p class="root">{home(report.findings.find((finding) => finding.assistantId === row.assistantId)?.root)}</p>{/snippet}
      </Row>
    {:else}
      <p>No assistant memory found.</p>
    {/each}
  {/if}
</Group>

<style>
  .root { margin: 0; color: var(--muted); font: var(--text-12) var(--font-mono); overflow-wrap: anywhere; }
</style>
