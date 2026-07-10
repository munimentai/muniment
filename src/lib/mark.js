// The milled ring at rest — design-spec §1.8 reference geometry:
// r(t) = 16.5 + 1.6·sin(22t) in a 48-unit viewBox, monoline stroke, round caps
// (engine: docs/design-reference/ring/muniment-ring-pulse-spin.html).
export function ringPath(points = 220, amplitude = 1, scale = 1) {
  const pts = []
  for (let i = 0; i <= points; i++) {
    const t = (2 * Math.PI * i) / points
    const r = (16.5 + 1.6 * amplitude * Math.sin(22 * t)) * scale
    pts.push(`${(24 + r * Math.cos(t)).toFixed(2)},${(24 + r * Math.sin(t)).toFixed(2)}`)
  }
  return `M${pts.join(' L')} Z`
}

// Filled two-edge reduction used at rendered sizes of 20px and below.
export function solidRingPath(points = 140) {
  const outer = []
  const inner = []
  for (let i = 0; i <= points; i++) {
    const t = (2 * Math.PI * i) / points
    const outerRadius = 19 + 1.7 * Math.sin(22 * t)
    const innerRadius = 12.5 + 1.1 * Math.sin(22 * t)
    outer.push(`${(24 + outerRadius * Math.cos(t)).toFixed(1)},${(24 + outerRadius * Math.sin(t)).toFixed(1)}`)
    inner.push(`${(24 + innerRadius * Math.cos(-t)).toFixed(1)},${(24 + innerRadius * Math.sin(-t)).toFixed(1)}`)
  }
  return `M${outer.join(' L')} Z M${inner.join(' L')} Z`
}

// A timestamp becomes animation time only relative to the shared document
// epoch. Components mounted later therefore join the same phase.
export function ringPhase(timestamp, epoch) {
  return Math.max(0, timestamp - epoch) / 1000
}

function noise(index, salt) {
  const value = Math.sin(index * 127.1 + salt * 311.7) * 43758.5453
  return value - Math.floor(value)
}

function signedDepth(beat) {
  const magnitude = 0.45 + noise(beat, 2) * 0.55
  return noise(beat, 3) < 0.6 ? magnitude : -magnitude
}

// Pure, deterministic port of the §1.8 motion grammar. Random-looking choices
// are derived from the shared phase, so every visible ring gets the same frame.
export function ringFrame(seconds) {
  let beat = 0
  let beatStart = 0
  const beatStarts = [0]
  let beatDuration = 1 / ((0.8 + noise(beat, 1) * 0.45) * 1.1)
  while (beatStart + beatDuration <= seconds) {
    beatStart += beatDuration
    beat += 1
    beatStarts.push(beatStart)
    beatDuration = 1 / ((0.8 + noise(beat, 1) * 0.45) * 1.1)
  }

  const beatPhase = (seconds - beatStart) / beatDuration
  const pulse = beatPhase < 0.32
    ? Math.sin((beatPhase / 0.32) * Math.PI / 2)
    : Math.cos(((beatPhase - 0.32) / 0.68) * Math.PI / 2)
  const flex = pulse * signedDepth(beat)

  const speeds = [0, 0, 0, 0.35, -0.35, 0.9, -0.9, 2.2, -2.2]
  let segment = 0
  let segmentStart = 0
  let duration = 1.5 + noise(segment, 4) * 2.5
  let angle = 0
  while (segmentStart + duration <= seconds) {
    angle += speeds[Math.floor(noise(segment, 5) * speeds.length)] * duration
    segmentStart += duration
    segment += 1
    duration = 1.5 + noise(segment, 4) * 2.5
  }
  const target = speeds[Math.floor(noise(segment, 5) * speeds.length)]
  const prior = segment === 0 ? 0 : speeds[Math.floor(noise(segment - 1, 5) * speeds.length)]
  const progress = (seconds - segmentStart) / duration
  const eased = progress * progress * (3 - 2 * progress)
  const omega = prior + (target - prior) * eased
  angle += ((prior + omega) / 2) * (seconds - segmentStart)

  // A trace begins every 5–10 beats and takes 0.9–1.4 seconds.
  let traceBeat = 5 + Math.floor(noise(0, 6) * 5)
  let trace = 0
  for (let traceIndex = 1; traceBeat <= beat; traceIndex += 1) {
    const traceDuration = 0.9 + noise(traceIndex, 7) * 0.5
    const elapsed = seconds - beatStarts[traceBeat]
    if (elapsed < traceDuration) trace = elapsed / traceDuration
    traceBeat += 5 + Math.floor(noise(traceIndex, 6) * 5)
  }

  return {
    amplitude: 1 + 0.6 * flex,
    scale: 1 + 0.032 * flex,
    strokeScale: 1 + 0.18 * flex,
    angle: angle * (180 / Math.PI),
    trace,
  }
}
