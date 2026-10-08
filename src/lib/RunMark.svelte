<script>
  import { onMount, untrack } from 'svelte'
  import { stageWord } from './chat-state.js'
  import { thinkingSettle } from './thinking-transition.js'
  import { MARK_EVENT, MARK_STORAGE_KEY, parseMark, readStoredMark } from './thinking-mark-state.js'
  import { DEGREE, trunkPath } from './elephant-trunk.js'
  import { EAR, HIND, HIP, TILT_DEG, TORSO, foreLeg, foreRise } from './pocket-fold.js'

  // The mark in flight: the pocket-fold elephant, and the word tracks what the
  // run does. A word holds at least this long, so a fast tool never flickers.
  const HOLD_MS = 800
  const FORE = foreLeg(0)
  const TRUNK = trunkPath()
  // The run's state picks the gait. The elephant stands while the model routes
  // or writes, walks while a tool runs, and runs through a burst of tools.
  // Elephants keep one walking footfall at every speed, so a run is the same
  // stride played faster. While the model thinks, the head rises and the trunk
  // curls up in front of the face with its tip hooked into a J, as elephants
  // raise it to sniff the air. The front of the body rises about the hind hip
  // and both feet stay down. A walk or a run keeps the front raised and the
  // trunk in the same J.
  const RATES = { rest: { body: 0, ear: 1 }, walk: { body: 1, ear: 2 }, run: { body: 2, ear: 4 } }
  const BURST_MS = 2000
  const CURL_MS = 700
  const STRIDE_MS = 1200
  const ease = (t) => t < 0.5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2
  let { stage = 'routing', mark = null, size = 20 } = $props()
  let chosen = $state(readStoredMark())
  const style = $derived(parseMark(mark ?? chosen))
  let shown = $state(stageWord(stage))
  let gait = $state('rest')
  let shownAt = Date.now()
  let walker = $state()
  let rearing = $state()
  let hind = $state()
  let fore = $state()
  let trunk = $state()
  let lastTool = 0
  let burstTimer

  onMount(() => {
    const follow = (event) => { chosen = parseMark(event.detail) }
    const stored = (event) => { if (event.key === MARK_STORAGE_KEY) chosen = parseMark(event.newValue) }
    window.addEventListener(MARK_EVENT, follow)
    window.addEventListener('storage', stored)
    return () => { window.removeEventListener(MARK_EVENT, follow); window.removeEventListener('storage', stored); clearTimeout(burstTimer) }
  })

  // A tool within two seconds of the last one is a burst, and the elephant runs
  // until the burst ends.
  $effect(() => {
    const now = String(stage)
    untrack(() => {
      clearTimeout(burstTimer)
      if (now.startsWith('tool:')) {
        const at = Date.now()
        gait = at - lastTool < BURST_MS ? 'run' : 'walk'
        lastTool = at
        burstTimer = setTimeout(() => { if (gait === 'run') gait = 'walk' }, BURST_MS)
      } else {
        gait = 'rest'
      }
    })
  })

  // A gait change speeds up or slows the running cycles, so no limb snaps to a new pose.
  $effect(() => {
    const rate = style === 'motion' ? RATES[gait] : RATES.rest
    for (const animation of walker?.getAnimations?.({ subtree: true }) ?? []) {
      animation.updatePlaybackRate(animation.animationName?.endsWith('ear-flap') ? rate.ear : rate.body)
    }
  })

  // The trunk and the head are drawn each frame. The trunk sways with the hind
  // leg's stride. While the model thinks or a tool runs, the front rises and the
  // trunk curls into the J, and both settle when the run stands.
  onMount(() => {
    if (typeof requestAnimationFrame !== 'function') return
    const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)')
    let frame
    let raised = 0
    let last = performance.now()
    // A change of indicator draws new elements, so the frame remembers which
    // elements it last drew on.
    let drawn = [null, '']
    let rose = [null, 0]
    const draw = (now) => {
      const moving = style === 'motion' && !reduced?.matches
      const step = (now - last) / CURL_MS
      last = now
      const thinking = moving && stage === 'thinking'
      raised = moving && (thinking || gait !== 'rest') ? Math.min(1, raised + step) : moving ? Math.max(0, raised - step) : 0
      const curl = ease(raised)
      const stride = moving ? hind?.getAnimations?.()[0]?.currentTime : null
      const sway = typeof stride === 'number' ? Math.sin(360 * DEGREE * stride / STRIDE_MS) * (1 - curl) : 0
      const d = trunkPath(curl, sway)
      if (trunk !== drawn[0] || d !== drawn[1]) { trunk?.setAttribute('d', d); drawn = [trunk, d] }
      rearing?.setAttribute('transform', curl ? `rotate(${-TILT_DEG * curl} ${HIP[0]} ${HIP[1]})` : '')
      const rise = foreRise(curl)
      if (fore !== rose[0] || rise !== rose[1]) {
        fore?.setAttribute('d', foreLeg(rise))
        fore?.style.setProperty('transform-origin', `50% ${40 / (161 + rise) * 100}%`)
        rose = [fore, rise]
      }
      frame = requestAnimationFrame(draw)
    }
    frame = requestAnimationFrame(draw)
    return () => cancelAnimationFrame(frame)
  })

  $effect(() => {
    const next = stageWord(stage)
    return untrack(() => {
      if (next === shown) return
      const wait = HOLD_MS - (Date.now() - shownAt)
      if (wait <= 0) {
        shown = next
        shownAt = Date.now()
        return
      }
      const timer = setTimeout(() => { shown = next; shownAt = Date.now() }, wait)
      return () => clearTimeout(timer)
    })
  })
</script>

<span class="thinking" out:thinkingSettle|global>{#if style === 'text'}<span class="stage-sheen">{shown}</span>{:else}<svg width={size} height={size} viewBox="-20 -20 566 499" aria-label={shown}><g class="walker" class:ear-only={style === 'ear'} class:still={style === 'still'} bind:this={walker}><path class="body leg hind" bind:this={hind} d={HIND} /><path class="body leg fore" bind:this={fore} d={FORE} /><g bind:this={rearing}><path class="body trunk" bind:this={trunk} d={TRUNK} /><path class="body torso" d={TORSO} /><path class="accent" d={EAR} aria-hidden="true" /></g></g></svg><span>{shown}</span>{/if}</span>

<style>
  .thinking { display: flex; align-items: center; gap: 9px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .thinking svg { overflow: visible; }
  .thinking .body { fill: var(--muted); }
  .thinking .accent { fill: var(--signal); }
  .torso, .trunk { shape-rendering: geometricPrecision; }
  /* Every cycle runs from the start. A standing elephant sets the body cycles' rate to zero, so it holds its pose. */
  .walker { transform-box: fill-box; transform-origin: center bottom; animation: vault .6s ease-in-out infinite; }
  /* The ear hinges on its diagonal edge, the line of the elephant's back that runs down over it. It unfolds out across that edge, away from the body, and folds back. The origin is the diagonal's midpoint. */
  .accent { transform-box: fill-box; transform-origin: 50% 50%; animation: ear-flap 3.2s infinite; }
  /* Each limb turns about the center of its round cap at the torso's underside. */
  .leg { transform-box: fill-box; }
  .hind { transform-origin: 50% 34.06%; animation: stride 1.2s infinite; }
  /* The hind leg leads and the fore leg follows a quarter cycle later, as elephants walk at every speed. */
  .fore { transform-origin: 50% 24.84%; animation: stride 1.2s infinite; animation-delay: -.9s; }
  .ear-only .leg, .walker.ear-only, .still .leg, .walker.still, .still .accent { animation: none; }
  /* The ear lifts slowly and whips out at the end, then falls back fast and settles. */
  @keyframes ear-flap {
    0%, 45%, 100% { transform: rotate(41.33deg) scaleY(1) rotate(-41.33deg); }
    45% { animation-timing-function: cubic-bezier(.7, 0, .84, 0); }
    70% { transform: rotate(41.33deg) scaleY(-.6) rotate(-41.33deg); animation-timing-function: cubic-bezier(.16, 1, .3, 1); }
  }
  /* The cycle starts mid-stance with the leg upright. Stance sweeps the foot back slowly. Swing brings it forward fast with the knee bent. */
  @keyframes stride {
    0% { transform: rotate(0deg) scaleY(1); animation-timing-function: linear; }
    31% { transform: rotate(8deg) scaleY(1); animation-timing-function: ease-in-out; }
    50% { transform: rotate(0deg) scaleY(.92); animation-timing-function: ease-in-out; }
    69% { transform: rotate(-8deg) scaleY(1); animation-timing-function: linear; }
    100% { transform: rotate(0deg) scaleY(1); }
  }
  /* The body rises a little over each planted foot. */
  @keyframes vault {
    0%, 100% { transform: translateY(0) rotate(0deg); }
    50% { transform: translateY(-1.5%) rotate(-.6deg); }
  }
  /* The text choice shows only the stage word, with the sheen the tool rows use. */
  .stage-sheen { background: linear-gradient(100deg, var(--muted) 35%, var(--ink) 50%, var(--muted) 65%); background-size: 250% 100%; background-clip: text; -webkit-background-clip: text; color: transparent; animation: stage-sheen 2.4s linear infinite; }
  @keyframes stage-sheen { from { background-position: 140% 0; } to { background-position: -40% 0; } }
  @media (prefers-reduced-motion: reduce) {
    .walker, .accent, .leg { animation: none !important; }
    .stage-sheen { animation: none; background: none; color: var(--muted); }
  }
  @media (forced-colors: active) { .stage-sheen { background: none; color: CanvasText; } }
</style>
