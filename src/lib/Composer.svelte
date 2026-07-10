<script>
  import { onMount } from 'svelte'

  let input

  onMount(() => input?.focus())
</script>

<div class="composer-wrap">
  <div class="composer">
    <textarea bind:this={input} aria-label="Message" placeholder="Ask muniment" rows="2"></textarea>
    <button disabled aria-label="Send message">Send</button>
  </div>
  <p>Routing is automatic. Every reply carries its receipt.</p>
</div>

<style>
  .composer-wrap { width: min(720px, 100%); }
  .composer {
    display: flex;
    align-items: flex-end;
    gap: 12px;
    padding: 12px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
  }
  .composer:focus-within { border-color: var(--muted); }
  textarea {
    flex: 1;
    min-width: 0;
    resize: none;
    border: 0;
    outline: 0;
    color: var(--ink);
    background: transparent;
    font: inherit;
    line-height: var(--leading-body);
  }
  textarea::placeholder { color: var(--muted); }
  button {
    padding: 6px 12px;
    color: var(--muted);
    background: var(--faint);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    font: inherit;
    font-size: var(--text-13);
    cursor: default;
  }
  p {
    margin-top: 7px;
    color: var(--muted);
    font-family: var(--font-mono);
    font-size: 11px;
  }
</style>
