<script>
  // macOS holds Muniment's reads of Home while its folder prompt waits, or
  // refuses them after Don't Allow. The sidebar names that state with core's
  // words and one Allow action until Home opens.
  import Button from './ui/Button.svelte'
  import LucideIcon from './LucideIcon.svelte'
  import { folderAccessHeading } from './folder-access.js'

  let { access, onallow = () => {} } = $props()
  let busy = $state(false)

  async function allow() {
    busy = true
    try { await onallow() } finally { busy = false }
  }
</script>

<section class="folder-access" aria-label="Folder access">
  <p class="head"><LucideIcon name="folder" size={14} /><strong>{folderAccessHeading(access)}</strong></p>
  <p>{access.message}</p>
  <div class="actions">
    <Button variant="primary" disabled={busy} onclick={allow}>Allow access</Button>
  </div>
</section>

<style>
  .folder-access { display: grid; gap: 8px; margin: 4px 8px 8px; padding: 10px; border: 1px solid var(--ochre); border-radius: var(--radius-control); background: var(--surface); }
  p { margin: 0; color: var(--muted); font-size: var(--text-12); line-height: 1.5; }
  .head { display: flex; align-items: center; gap: 6px; color: var(--ochre); }
  .head strong { color: var(--ink); font-weight: 600; }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; }
</style>
