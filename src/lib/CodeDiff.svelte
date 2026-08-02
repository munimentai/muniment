<script>
  import DOMPurify from 'dompurify'
  import { html } from 'diff2html'
  import { codeDiffPresentation } from './code-diff.js'

  let { codeDiff } = $props()

  let presentation = $derived(codeDiffPresentation(codeDiff))
  let records = $derived.by(() => {
    const textFiles = presentation.renderer.input.map((file) => ({
      kind: 'text',
      position: file.position,
      html: DOMPurify.sanitize(html([file], presentation.renderer.options), {
        ALLOWED_ATTR: ['class', 'colspan', 'title'],
        ALLOWED_TAGS: ['del', 'div', 'ins', 'span', 'table', 'tbody', 'td', 'tr'],
        ALLOW_ARIA_ATTR: false,
        ALLOW_DATA_ATTR: false,
      }),
    }))
    const binaryFiles = presentation.binaryFiles.map((file) => ({
      kind: 'binary',
      ...file,
    }))

    return [...textFiles, ...binaryFiles].sort((left, right) => left.position - right.position)
  })
</script>

<section class="code-diff" aria-label="Code changes" data-diff-id={presentation.id}>
  {#if presentation.truncatedWarning}
    <p class="code-diff-warning" role="status">{presentation.truncatedWarning}</p>
  {/if}

  {#if presentation.emptyMessage}
    <p class="code-diff-empty">{presentation.emptyMessage}</p>
  {:else}
    {#each records as record (record.position)}
      {#if record.kind === 'text'}
        <div class="code-diff-file">{@html record.html}</div>
      {:else}
        <article class="code-diff-binary">
          <p class="code-diff-path">
            {record.oldName === record.newName ? record.newName : `${record.oldName} → ${record.newName}`}
          </p>
          <p>{record.message}</p>
        </article>
      {/if}
    {/each}
  {/if}
</section>

<style>
  .code-diff {
    max-width: 100%;
    color: var(--ink);
    font: var(--text-13) var(--font-mono);
  }

  .code-diff-warning,
  .code-diff-empty,
  .code-diff-binary {
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
  }

  .code-diff-warning {
    margin-bottom: 8px;
    border-left: 3px solid var(--ochre);
    color: var(--ink);
  }

  .code-diff-file,
  .code-diff-binary {
    margin-bottom: 8px;
  }

  .code-diff-file:last-child,
  .code-diff-binary:last-child {
    margin-bottom: 0;
  }

  .code-diff-path {
    margin-bottom: 4px;
    color: var(--ink);
  }

  .code-diff-binary p:last-child {
    color: var(--muted);
  }
</style>
