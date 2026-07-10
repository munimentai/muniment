<script>
  import { onMount } from 'svelte'

  import { ringFrame, ringPath, ringPhase, solidRingPath } from './mark.js'

  let { state: ringState = 'rest', size = 34, strokeWidth = 4.5, label = 'muniment' } = $props()
  let frame = $state(ringFrame(0))
  let reducedMotion = $state(false)
  const epoch = typeof performance === 'undefined' ? 0 : performance.timeOrigin
  const solidPath = solidRingPath()

  let small = $derived(size <= 20)
  let thinking = $derived(ringState === 'thinking')
  let animated = $derived(thinking && !reducedMotion)
  let bodyPath = $derived(animated && !small ? ringPath(220, frame.amplitude, frame.scale) : ringPath())
  let width = $derived(animated ? strokeWidth * frame.strokeScale : strokeWidth)
  let rotation = $derived(animated ? `rotate(${frame.angle.toFixed(2)} 24 24)` : undefined)
  let traceOpacity = $derived(animated && frame.trace ? Math.sin(frame.trace * Math.PI) * 0.9 : 0)
  let traceOffset = $derived(-frame.trace * 207.55 * 1.16)

  onMount(() => {
    const media = matchMedia('(prefers-reduced-motion: reduce)')
    const updatePreference = () => (reducedMotion = media.matches)
    updatePreference()
    media.addEventListener('change', updatePreference)

    return () => {
      media.removeEventListener('change', updatePreference)
    }
  })

  $effect(() => {
    if (!thinking || reducedMotion) return

    let request
    const tick = (now) => {
      frame = ringFrame(ringPhase(performance.timeOrigin + now, epoch))
      request = requestAnimationFrame(tick)
    }
    request = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(request)
  })
</script>

<svg
  class:thinking
  width={size}
  height={size}
  viewBox="0 0 48 48"
  role="img"
  aria-label={label}
>
  <g transform={rotation}>
    {#if small}
      <path class="body solid" d={solidPath} fill-rule="evenodd" />
    {:else}
      <path class="body" d={bodyPath} stroke-width={width} />
      {#if thinking}
        <path
          class="trace"
          d={bodyPath}
          stroke-width={width * 1.15}
          stroke-dasharray="33.21 207.55"
          stroke-dashoffset={traceOffset}
          opacity={traceOpacity}
        />
      {/if}
    {/if}
  </g>
</svg>

<style>
  svg {
    display: block;
    overflow: visible;
  }

  path {
    fill: none;
    stroke: var(--ink);
    stroke-linecap: round;
  }

  svg.thinking path {
    stroke: var(--signal);
  }

  path.solid {
    fill: var(--ink);
    stroke: none;
  }

  svg.thinking path.solid {
    fill: var(--signal);
  }

  .trace {
    fill: none;
    pointer-events: none;
  }
</style>
