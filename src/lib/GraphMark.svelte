<script>
  import { pocketBody, pocketEar } from './pocket-fold.js'
  let { size = 160 } = $props()
</script>
<svg width={size} height={size} viewBox="-20 -54 566 566" aria-hidden="true"><path fill="currentColor" d={pocketBody} /><path fill="#2A7264" d={pocketEar} /></svg>
