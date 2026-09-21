<script>
  let { load, properties = {} } = $props()
  const component = $derived(load())
</script>
{#await component}<p role="status">Loading settings…</p>
{:then module}{@const Section = module.default}<Section {...properties} />
{:catch}<p role="alert">This section could not load. Choose another section, then retry.</p>{/await}
