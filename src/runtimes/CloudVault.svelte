<script>
  // Account → MuniCloud: one switch keeps the model connections this
  // computer shares in MuniCloud, which holds them encrypted and renews
  // sign-ins while this computer is off. It needs Share model connections on
  // Machines, which core answers through `fleet.credentials`.
  import { onMount } from 'svelte'
  import Group from '../lib/ui/Group.svelte'
  import Row from '../lib/ui/Row.svelte'
  import Toggle from '../lib/Toggle.svelte'
  import { agentRequest } from '../agents/agent-runtime.js'
  import { errorLine } from './runtimes.js'

  let { tauri, listen = (...args) => window.__TAURI__?.event?.listen(...args) } = $props()
  let list = $state(null)
  let shareModels = $state(null)
  let busy = $state(false)
  let error = $state('')

  onMount(() => {
    let alive = true
    let stop
    Promise.resolve(tauri.invoke('runtime_list')).then((next) => { if (alive) list = next }).catch(() => {})
    Promise.resolve(agentRequest(tauri, 'local', 'fleet.credentials', {})).then((answer) => { if (alive) shareModels = !!answer?.share_models }).catch(() => { shareModels = false })
    Promise.resolve(listen?.('runtimes-changed', ({ payload }) => { if (payload?.runtimes) list = payload }))
      .then((unlisten) => { if (alive) stop = unlisten; else unlisten?.() })
      .catch(() => {})
    return () => { alive = false; stop?.() }
  })

  async function setCloud(cloud) {
    busy = true
    error = ''
    try {
      const next = await tauri.invoke('runtime_share_models', { cloud })
      if (next?.runtimes) list = next
    } catch (failure) {
      error = errorLine(failure)
    } finally {
      busy = false
    }
  }
</script>

<Group label="Shared model connections">
  <Row title="Keep model connections in MuniCloud" info={shareModels === false
    ? 'Turn on Share model connections in Machines first. MuniCloud then holds them encrypted and renews sign-ins while this computer is off.'
    : 'MuniCloud holds them encrypted and renews sign-ins while this computer is off. A free account shares with up to five machines.'}>
    <Toggle checked={!!list?.shareViaCloud} label="Keep model connections in MuniCloud" disabled={!list || !shareModels || busy} onchange={setCloud} />
  </Row>
  {#if list?.shareError}<p role="alert">{list.shareError}</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</Group>
