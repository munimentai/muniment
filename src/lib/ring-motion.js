// The mark's thinking motion, one organism shared by every mark on screen so
// all of them breathe and turn together. Breath is an irregular heartbeat with
// a signed depth: a swell sharpens the teeth, a slim smooths them, and scale,
// band width and milling depth flex as one. Spin eases its angular velocity
// toward a random target every 1.5 to 4 seconds, from still, slow, medium or
// fast in either direction, with still weighted heaviest. A trace runs the
// band's centreline once every 5 to 10 beats. Reduced motion never starts the
// loop, so a subscriber keeps the still pose it drew at mount.
import { sealBandPath, sealTracePath } from './mark.js'

const rand = (a, b) => a + Math.random() * (b - a)
const pick = (list) => list[Math.floor(Math.random() * list.length)]
// rad/s, still weighted
const SPEEDS = [0, 0, 0, 0.35, -0.35, 0.9, -0.9, 2.2, -2.2]
const FRAME = 1 / 60

const subscribers = new Set()
let running = false
let beatPhase = 0
let beatRate = rand(0.85, 1.2)
let depth = rand(0.5, 1) * pick([1, 1, -1])
let beats = 0
let traceAt = Math.floor(rand(5, 10))
let tracing = null
let angle = 0
let omega = 0
let omegaTarget = 0
let retargetAt = 0

export const REST_POSE = Object.freeze({ scale: 1, ampMul: 1, widthMul: 1, angle: 0, trace: null })

export function reducedMotion() {
  return globalThis.matchMedia?.('(prefers-reduced-motion: reduce)')?.matches ?? false
}

// One frame of the organism. Exported so a test can drive it without a clock.
export function step(now) {
  if (now >= retargetAt) {
    omegaTarget = pick(SPEEDS)
    retargetAt = now + rand(1500, 4000)
  }
  omega += (omegaTarget - omega) * 0.03
  angle = (angle + omega * FRAME * (180 / Math.PI)) % 360

  beatPhase += FRAME * beatRate * 1.1
  if (beatPhase >= 1) {
    beatPhase -= 1
    beats += 1
    beatRate = rand(0.8, 1.25)
    // swell slightly favored
    depth = rand(0.45, 1) * pick([1, 1, 1, -1, -1])
    if (beats >= traceAt && !tracing) {
      tracing = { t: 0, dur: rand(0.9, 1.4) }
      traceAt = beats + Math.floor(rand(5, 10))
    }
  }
  // quick flex, slow settle
  const p = beatPhase < 0.32
    ? Math.sin(beatPhase / 0.32 * Math.PI / 2)
    : Math.cos((beatPhase - 0.32) / 0.68 * Math.PI / 2)
  const flex = p * depth
  const pose = {
    scale: 1 + 0.032 * flex,
    ampMul: 1 + 0.6 * flex,
    widthMul: 1 + 0.18 * flex,
    angle,
    trace: tracing ? Math.min(1, tracing.t / tracing.dur) : null,
  }
  if (tracing) {
    tracing.t += FRAME
    if (tracing.t >= tracing.dur) tracing = null
  }
  return pose
}

function frame(now) {
  if (!subscribers.size) {
    running = false
    return
  }
  const pose = step(now)
  for (const subscriber of subscribers) subscriber(pose)
  requestAnimationFrame(frame)
}

// Subscribe a mark to the organism. Returns the unsubscribe. The loop starts
// with the first subscriber and stops with the last, and never starts under
// reduced motion or without an animation clock.
export function subscribe(subscriber) {
  subscribers.add(subscriber)
  if (!running && !reducedMotion() && typeof requestAnimationFrame === 'function') {
    running = true
    requestAnimationFrame(frame)
  }
  return () => { subscribers.delete(subscriber) }
}

// Apply a pose to a mark's group, body and accent paths at a base band width.
export function paint(pose, { group, body, accent, width }) {
  const w = width * pose.widthMul
  body.setAttribute('d', sealBandPath(pose.ampMul, pose.scale, w))
  group.setAttribute('transform', `rotate(${pose.angle.toFixed(2)} 24 24)`)
  if (pose.trace === null) {
    accent.style.opacity = 0
    return
  }
  accent.setAttribute('d', sealTracePath(pose.ampMul, pose.scale))
  accent.setAttribute('stroke-width', (w * 1.15).toFixed(2))
  const length = accent.getTotalLength?.() ?? 0
  const segment = length * 0.16
  accent.style.strokeDasharray = `${segment} ${length}`
  accent.style.strokeDashoffset = -pose.trace * length * 1.16
  accent.style.opacity = Math.sin(pose.trace * Math.PI) * 0.9
}
