<script>
  // While dictation listens, the composer's top border becomes a thin wave that
  // follows the speaker's voice. The levels come from the native capture as
  // `level` dictation events, about 30 a second. The frame loop runs only while
  // dictation listens or the wave fades out after it stops, then the canvas
  // clears and the plain border shows again. It always animates: it is the live
  // reading of the voice, as the sidebar ring is of a run.
  import { onMount } from 'svelte'
  import { createVoiceWaveMotion, drawVoiceWave } from './voice-wave.js'

  let {
    active = false,
    listen,
    random = Math.random,
    requestFrame = (run) => requestAnimationFrame(run),
    cancelFrame = (id) => cancelAnimationFrame(id),
    context = (node) => node.getContext('2d'),
  } = $props()

  let canvas = $state()
  let ctx
  let level = 0
  let frame
  let last
  let size = ''
  const motion = createVoiceWaveMotion({ random })

  onMount(() => {
    let stopped = false
    let unlisten
    Promise.resolve(listen?.('dictation-event', ({ payload }) => {
      if (payload?.type === 'level' && Number.isFinite(payload.level)) level = Math.max(0, Math.min(1, payload.level))
    })).then((stop) => { if (stopped) stop?.(); else unlisten = stop }, () => {})
    return () => { stopped = true; unlisten?.(); if (frame !== undefined) cancelFrame(frame); frame = undefined }
  })

  $effect(() => {
    if (!active) return
    level = 0
    if (frame === undefined) { last = undefined; frame = requestFrame(draw) }
  })

  // Sizes the canvas backing store to its box at the device pixel ratio.
  function fit() {
    const ratio = window.devicePixelRatio || 1
    const width = canvas.clientWidth, height = canvas.clientHeight
    const next = `${width}x${height}@${ratio}`
    if (next !== size) {
      size = next
      canvas.width = Math.round(width * ratio)
      canvas.height = Math.round(height * ratio)
    }
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0)
    return { width, height }
  }

  function draw(now) {
    frame = undefined
    if (!canvas) return
    ctx ??= context(canvas)
    if (!ctx) return
    const dt = last === undefined ? 1 / 60 : Math.min(0.05, Math.max(0, (now - last) / 1000))
    last = now
    motion.step(dt, active ? level : 0)
    const { width, height } = fit()
    if (!active && motion.settled()) {
      ctx.clearRect(0, 0, width, height)
      motion.loud = 0
      motion.blend = 0
      return
    }
    const composer = canvas.parentElement
    const style = getComputedStyle(composer)
    drawVoiceWave(ctx, {
      width,
      height,
      radius: parseFloat(style.borderTopLeftRadius) || 18,
      border: style.borderTopColor,
      accent: getComputedStyle(canvas).getPropertyValue('--accent').trim() || getComputedStyle(document.documentElement).getPropertyValue('--accent').trim(),
      motion,
    })
    frame = requestFrame(draw)
  }
</script>

<canvas bind:this={canvas} class="voice-wave-line" data-testid="voice-wave-line" data-active={active || undefined} aria-hidden="true"></canvas>

<style>
  /* Centered on the composer's 1px top border, across its full outer width. */
  .voice-wave-line { position: absolute; left: -1px; top: -24.5px; width: calc(100% + 2px); height: 48px; pointer-events: none; }
</style>
