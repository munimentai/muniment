<script>
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { resultLeft, resultTone, travelTone } from './status-ring.js'
  // A thread row's leading mark. While core reports a status, Lucide's dashed
  // circle takes the place of the row's own icon, the children. The status in
  // core's words is the ring's text for screen readers, and a result keeps it
  // after its ring goes, so the row's description always holds it. With no
  // children and no ring the slot takes no width, so the title beside it
  // slides over as the ring comes and goes. `key` names the result, so each
  // result lights once.
  let { status = 'read', label = '', key = '', size = 14, id = undefined, children = undefined } = $props()
  const travel = $derived(travelTone(status))
  const result = $derived(resultTone(status))
  let lit = $state(false)
  $effect(() => {
    if (!result) { lit = false; return }
    const left = resultLeft(`${key}:${status}`)
    lit = left > 0
    if (!left) return
    const timer = setTimeout(() => { lit = false }, left)
    return () => clearTimeout(timer)
  })
  const tone = $derived(travel ?? (lit ? result : null))
</script>

<span class="status-lead" {id} data-empty={!tone && !children ? 'true' : undefined} style:--ring-size="{size}px">{#if tone}<span class="status-ring" aria-hidden="true" data-tone={tone} data-phase={travel ? 'travel' : 'lit'}><LucideIcon name="circle-dashed" {size} variant="action" /></span>{:else}{@render children?.()}{/if}{#if label && (travel || result)}<span class="visually-hidden">{label}</span>{/if}</span>

<style>
  /* The slot keeps the mark's size. The doubled class outranks the base reduced motion rule, so the slide always plays. */
  .status-lead.status-lead { flex: none; display: inline-flex; align-items: center; justify-content: center; min-width: var(--ring-size); height: var(--ring-size); transition: min-width .16s ease, margin .16s ease; transition-duration: .16s !important; }
  .status-lead.status-lead[data-empty] { min-width: 0; width: 0; margin: 0 !important; overflow: hidden; }
  .status-ring { display: inline-flex; }
  .status-ring[data-tone="success"] { --ring-tone: var(--success); }
  .status-ring[data-tone="warning"] { --ring-tone: var(--warning); }
  .status-ring[data-tone="danger"] { --ring-tone: var(--danger); }
  .status-ring[data-tone] :global(path) { stroke: var(--ring-tone); stroke-width: 2.4; }
  /* One dash at a time lights and fades behind, so the light travels clockwise.
     The ring moves under every Motion setting: the restated duration and count
     outrank the base reduced motion rule. */
  .status-ring[data-phase="travel"] :global(path) { opacity: .22; animation: ring-travel 1.2s linear infinite; animation-duration: 1.2s !important; animation-iteration-count: infinite !important; }
  .status-ring[data-phase="travel"] :global(path:nth-child(2)) { animation-delay: -1.05s; }
  .status-ring[data-phase="travel"] :global(path:nth-child(3)) { animation-delay: -.9s; }
  .status-ring[data-phase="travel"] :global(path:nth-child(4)) { animation-delay: -.75s; }
  .status-ring[data-phase="travel"] :global(path:nth-child(5)) { animation-delay: -.6s; }
  .status-ring[data-phase="travel"] :global(path:nth-child(6)) { animation-delay: -.45s; }
  .status-ring[data-phase="travel"] :global(path:nth-child(7)) { animation-delay: -.3s; }
  .status-ring[data-phase="travel"] :global(path:nth-child(8)) { animation-delay: -.15s; }
  .status-ring[data-phase="lit"] :global(path) { opacity: 1; }
  @keyframes ring-travel { 0%, 12% { opacity: 1; } 55%, 100% { opacity: .22; } }
</style>
