<script>
  let { tauri, settings } = $props()
  let sample = $state('')
  let pending = $state(false)
  let result = $state(null)
  let error = $state('')
  async function test() {
    pending = true; result = null; error = ''
    try { result = await tauri.invoke('model_router_test_route', { sample: sample.trim() }) }
    catch (failure) { error = String(failure?.message ?? failure) }
    finally { pending = false }
  }
</script>
<section aria-labelledby="test-routing-title">
  <h4 id="test-routing-title">Test routing</h4>
  <p>The sample reaches your saved classifier. This test does not generate a reply or run tools.</p>
  <label for="routing-sample">Sample request</label>
  <textarea id="routing-sample" rows="3" placeholder="Describe a task you would ask Muniment to handle." bind:value={sample} disabled={pending}></textarea>
  <button disabled={pending || !sample.trim() || (!settings?.options?.length || !settings?.enabled)} onclick={test}>{pending ? 'Testing…' : 'Test routing'}</button>
  {#if !settings?.enabled}<p>Turn on account balancing before testing.</p>{:else if !settings?.options?.length}<p>Connect an eligible account before testing.</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
  {#if result}<dl aria-label="Routing test result"><div><dt>Selected model</dt><dd>{result.model}</dd></div><div><dt>Decision</dt><dd>{result.reason}</dd></div><div><dt>Classification time</dt><dd>{result.elapsed_ms} ms</dd></div>{#if result.confidence !== null}<div><dt>Routing confidence</dt><dd>{Math.round(result.confidence * 100)}%</dd></div>{/if}</dl>{/if}
</section>
<style>
  section { display: grid; gap: 10px; border-top: 1px solid var(--border); padding-top: 22px; }
  h4 { margin: 0; font-size: var(--text-17); font-weight: 600; } p { margin: 0; color: var(--muted); font-size: var(--text-13); }
  label { font-size: var(--text-13); } textarea { width: 100%; box-sizing: border-box; padding: 10px; color: var(--ink); background: var(--paper); border: 1px solid var(--border); border-radius: var(--radius-control); font: inherit; font-size: var(--text-13); resize: vertical; }
  button { justify-self: start; padding: 6px 12px; border: 1px solid var(--border); border-radius: var(--radius-control); color: var(--ink); background: var(--surface); font: inherit; font-size: var(--text-13); cursor: pointer; } button:disabled { color: var(--muted); cursor: default; }
  dl { display: grid; gap: 10px; margin: 0; padding: 12px; background: var(--faint); border-radius: var(--radius-control); } dt { font-size: var(--text-13); color: var(--muted); } dd { margin: 4px 0 0; font: var(--text-12) var(--font-mono); overflow-wrap: anywhere; }
</style>
