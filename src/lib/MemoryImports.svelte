<script>
  import { open } from '@tauri-apps/plugin-dialog'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import Button from './ui/Button.svelte'
  import { COPY_LIMITS, copyFits, scanRows } from './onboarding-scan.js'

  // Memory that other assistants keep on this computer. The scan reads file
  // names, never bodies, and runs only when asked, because some roots sit
  // under Documents and a launch must not raise a folder prompt. Import copies
  // one store under memory/imports/ in Home. The panel shows names and counts,
  // never the text of a file.
  let { tauri } = $props()
  let report = $state(null)
  let scanning = $state(false)
  let error = $state('')
  let imports = $state({})
  let archive = $state(null)
  const rows = $derived(scanRows(report).filter((row) => row.fileCount > 0))
  const zip = $derived(archive?.manifest ? zipPlan(archive.manifest) : null)
  const home = (path) => path?.replace(/^\/(Users|home)\/[^/]+/, '~').replace(/^[A-Z]:\\Users\\[^\\]+/, '~')
  const files = (count) => (count === 1 ? '1 file' : `${count} files`)
  const TOO_LARGE = `Too large to copy. Copy takes up to ${COPY_LIMITS.files} files.`
  const HOME_ERRORS = ['invalidInput', 'destinationConflict', 'secretRejected', 'saveFailed']
  const REASONS = { secret: 'holds a secret.', notText: 'is not text.', tooLarge: 'is over 64 KB.', unreadable: 'could not be read.' }

  async function scan() {
    scanning = true
    error = ''
    imports = {}
    archive = null
    try {
      report = await tauri.invoke('onboarding_scan')
    } catch (_) {
      error = 'Muniment could not look for assistant memory. Try again.'
    } finally {
      scanning = false
    }
  }

  function assistantError(row, failure) {
    switch (failure?.kind) {
      case 'tooLarge': return TOO_LARGE
      case 'nothingFound': return `Muniment found no ${row.displayName} memory files. Look again.`
      case 'unknownAssistant': return `Muniment does not know ${row.displayName}.`
      case 'invalidInput': return `Muniment could not find the ${row.displayName} memory folder.`
      default: return `Muniment could not copy the ${row.displayName} files. Try again.`
    }
  }

  // Look again starts a new set of results, so a late answer lands nowhere.
  async function importAssistant(row) {
    const results = imports
    if (results[row.assistantId]?.pending) return
    results[row.assistantId] = { pending: true }
    try {
      results[row.assistantId] = { result: await tauri.invoke('memory_import_assistant', { assistantId: row.assistantId }) }
    } catch (failure) {
      results[row.assistantId] = { error: assistantError(row, failure) }
    }
  }

  // The write plan adds a short header to each file, so a file within 1 KB of
  // the document cap stays out.
  function zipPlan(manifest) {
    const selected = manifest.entries.filter((entry) => entry.byteSize <= COPY_LIMITS.document - 1024)
    const leftOut = manifest.entries.filter((entry) => !selected.includes(entry))
      .map((entry) => ({ sourceName: entry.name, reason: 'tooLarge' }))
    const bytes = selected.reduce((total, entry) => total + entry.byteSize, 0)
    const fits = selected.length > 0 && selected.length <= COPY_LIMITS.files && bytes <= COPY_LIMITS.bytes
    let description = ''
    if (!manifest.entries.length) description = 'This ZIP holds no text files.'
    else if (!selected.length) description = 'Every file in this ZIP is over 64 KB.'
    else if (!fits) description = TOO_LARGE
    return { selected: selected.map((entry) => entry.name), leftOut, fits, description }
  }

  function previewError(name, failure) {
    switch (failure?.kind) {
      case 'notFound': return `Muniment could not find ${name}.`
      case 'archiveTooLarge': return `${name} is over 64 MB.`
      case 'invalidArchive': return `${name} is not a ZIP file Muniment can read.`
      case 'tooManyEntries': return `${name} holds too many files.`
      case 'entryTooLarge':
      case 'totalTooLarge': return `${name} expands to too much text.`
      case 'encrypted': return `${name} is encrypted. Export it again without a password.`
      case 'unsupportedEntry': return `${name} holds a file that is not text, Markdown or JSON.`
      case 'invalidText': return `${name} holds a text file that is not UTF-8.`
      case 'pathTraversal':
      case 'pathTooDeep':
      case 'duplicateEntry':
      case 'symlink': return `${name} holds an entry Muniment does not open.`
      default: return `Muniment could not read ${name}. Try again.`
    }
  }

  async function pickArchive() {
    let path
    try {
      path = await open({ multiple: false, directory: false, filters: [{ name: 'ZIP', extensions: ['zip'] }] })
    } catch (_) {
      return
    }
    if (!path) return
    const name = path.split(/[\\/]/).pop()
    archive = { path, name, previewing: true }
    try {
      const manifest = await tauri.invoke('onboarding_import_preview', { archivePath: path })
      if (archive?.path === path) archive = { path, name, manifest }
    } catch (failure) {
      if (archive?.path === path) archive = { path, name, previewError: previewError(name, failure) }
    }
  }

  async function importArchive() {
    const current = archive
    if (current.pending) return
    const { path, name } = current
    const { selected } = zip
    const leftOut = [...zip.leftOut]
    current.pending = true
    current.error = ''
    current.result = null
    try {
      let entries = await tauri.invoke('onboarding_import_extract', { archivePath: path, selectedNames: selected })
      const { homePath } = await tauri.invoke('home_status')
      // A file the write plan reads as a secret stays out by name, and the rest copy.
      while (entries.length) {
        try {
          const saved = await tauri.invoke('home_confirm_import', { homePath, approvedEntries: entries })
          current.result = { importedFileCount: saved.importedFileCount, alreadyCopiedCount: saved.alreadyCopiedCount, leftOut, destination: 'memory/imports/export' }
          return
        } catch (failure) {
          const secret = failure?.kind === 'secretRejected' && entries.find((entry) => entry.sourceName === failure.sourceName)
          if (!secret) throw failure
          leftOut.push({ sourceName: secret.sourceName, reason: 'secret' })
          entries = entries.filter((entry) => entry !== secret)
        }
      }
      current.result = { importedFileCount: 0, alreadyCopiedCount: 0, leftOut, destination: 'memory/imports/export' }
    } catch (failure) {
      if (failure?.kind === 'tooLarge') current.error = TOO_LARGE
      else if (failure?.kind && !HOME_ERRORS.includes(failure.kind)) current.error = previewError(name, failure)
      else current.error = `Muniment could not copy the files in ${name}. Try again.`
    } finally {
      current.pending = false
    }
  }
</script>

{#snippet outcome(copy)}
  {#if copy?.pending}
    <p class="note" role="status">Copying files…</p>
  {:else if copy?.error}
    <p class="note" role="alert">{copy.error}</p>
  {:else if copy?.result}
    {@const { importedFileCount: copied, alreadyCopiedCount: already, leftOut, destination } = copy.result}
    {#if copied}
      <p class="note">Copied <span class="mono">{copied}</span> {copied === 1 ? 'file' : 'files'} to <span class="mono">{destination}</span>.</p>
    {:else}
      <p class="note">Nothing new to copy.</p>
    {/if}
    {#if already}
      <p class="note"><span class="mono">{already}</span> {already === 1 ? 'file was' : 'files were'} already copied.</p>
    {/if}
    {#if leftOut.length}
      <ul class="left-out">
        {#each leftOut as file, index (index)}
          <li><span class="mono">{file.sourceName}</span> {REASONS[file.reason] ?? REASONS.unreadable}</li>
        {/each}
      </ul>
    {/if}
  {/if}
{/snippet}

<Group heading="Imports" info="Notes that Claude Code, Codex, Obsidian and other assistants keep on this computer.">
  {#snippet actions()}
    <Button disabled={scanning} onclick={scan}>{report ? 'Look again' : 'Look for memory'}</Button>
    <Button disabled={archive?.previewing || archive?.pending} onclick={pickArchive}>Import a ZIP…</Button>
  {/snippet}
  {#if scanning}
    <p role="status">Looking for assistant memory…</p>
  {:else if error}
    <p role="alert">{error}</p>
  {:else if !report}
    <p>Bring in notes from other assistants.</p>
  {:else}
    {#each rows as row (row.assistantId)}
      {@const copy = imports[row.assistantId]}
      <Row title={row.displayName} label={row.displayName} meta={files(row.fileCount)} description={!copyFits(row) ? TOO_LARGE : row.capped ? 'The count may be incomplete.' : ''}>
        {#snippet detail()}
          <p class="root">{home(report.findings.find((finding) => finding.assistantId === row.assistantId)?.root)}</p>
          {@render outcome(copy)}
        {/snippet}
        {#if copyFits(row)}
          <Button disabled={copy?.pending} onclick={() => importAssistant(row)}>{copy?.error ? 'Import again' : 'Import'}</Button>
        {/if}
      </Row>
    {:else}
      <p>No assistant memory found.</p>
    {/each}
  {/if}
  {#if archive}
    <Row title={archive.name} label={archive.name} meta={archive.manifest ? files(archive.manifest.entries.length) : ''} description={zip?.description ?? ''}>
      {#snippet detail()}
        <p class="root">{home(archive.path)}</p>
        {#if archive.previewing}
          <p class="note" role="status">Reading the ZIP…</p>
        {:else if archive.previewError}
          <p class="note" role="alert">{archive.previewError}</p>
        {:else}
          {@render outcome(archive)}
        {/if}
      {/snippet}
      {#if zip?.fits}
        <Button disabled={archive.pending} onclick={importArchive}>{archive.error ? 'Import again' : 'Import'}</Button>
      {/if}
    </Row>
  {/if}
</Group>

<style>
  .root { margin: 0; color: var(--muted); font: var(--text-12) var(--font-mono); overflow-wrap: anywhere; }
  .note { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .mono { font: var(--text-12) var(--font-mono); overflow-wrap: anywhere; }
  .left-out { display: grid; gap: 2px; margin: 0; padding: 0; list-style: none; color: var(--muted); font-size: var(--text-13); }
</style>
