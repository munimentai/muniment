// The voice wave on the composer's top edge: its motion from the speaker's
// loudness, and its drawing on a canvas centered on the border line.

// The warm color the wave's middle blends toward while the person speaks.
export const VOICE_WAVE_WARM = '#F2E36B'

// Below this smoothed loudness a stopped wave has faded out.
const SETTLED = 0.002

// The RGB of a hex or rgb() color, or null for another form.
export function parseColor(value) {
  const text = String(value ?? '').trim()
  let match = /^#([0-9a-f]{3})$/i.exec(text)
  if (match) return [...match[1]].map((digit) => parseInt(digit + digit, 16))
  match = /^#([0-9a-f]{6})(?:[0-9a-f]{2})?$/i.exec(text)
  if (match) return [0, 2, 4].map((at) => parseInt(match[1].slice(at, at + 2), 16))
  match = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/i.exec(text)
  if (match) return match.slice(1, 4).map((part) => Math.round(Number(part)))
  return null
}

// The wave's state between frames: the smoothed loudness, the warm blend, the
// ripple phase and the center the ripples gather around.
export function createVoiceWaveMotion({ random = Math.random } = {}) {
  const motion = { loud: 0, blend: 0, phase: 0, center: 0.5, centerTarget: 0.5 }

  // Advances the motion by `dt` seconds toward the loudness `target`, from 0 to 1.
  motion.step = (dt, target) => {
    // A quick rise and a slow fall, so the wave swells on each syllable and settles between words.
    const k = target > motion.loud ? 1 - Math.exp(-dt / 0.07) : 1 - Math.exp(-dt / 0.38)
    motion.loud += (target - motion.loud) * k
    // The middle warms with the voice and cools back to the accent in silence.
    motion.blend += (Math.min(1, motion.loud * 2) - motion.blend) * (1 - Math.exp(-dt / 0.6))
    // The ripples gather in one region that drifts slowly, and settle back to the middle in silence.
    if (random() < dt * 0.35) motion.centerTarget = 0.3 + random() * 0.4
    motion.center += ((motion.loud > 0.08 ? motion.centerTarget : 0.5) - motion.center) * (1 - Math.exp(-dt / 2.2))
    motion.phase += dt * (0.9 + motion.loud * 1.4)
    return motion
  }

  // A stopped wave has faded out once its loudness settles.
  motion.settled = () => motion.loud < SETTLED

  return motion
}

// Draws the wave on `ctx`, `width` by `height` CSS pixels with the border line
// at its middle. The line runs along the straight top edge between the
// rounded corners of `radius`. At each end it starts in the `border` color, so
// the border flows around the corner, then blends into `accent` over the fade.
export function drawVoiceWave(ctx, { width, height, radius, border, accent, motion }) {
  ctx.clearRect(0, 0, width, height)
  const mid = height / 2
  const [ar, ag, ab] = parseColor(accent) ?? [0, 0, 0]
  const [wr, wg, wb] = parseColor(VOICE_WAVE_WARM)
  const t = motion.blend * 0.55
  const warm = `rgb(${ar + (wr - ar) * t | 0}, ${ag + (wg - ag) * t | 0}, ${ab + (wb - ab) * t | 0})`
  const clear = `rgba(${ar}, ${ag}, ${ab}, 0)`
  const x0 = radius, x1 = width - radius, span = x1 - x0
  if (span <= 0) return
  const fade = Math.min(140, span * 0.18)
  const stop = (x) => Math.max(0, Math.min(1, (x - x0) / span))
  const base = ctx.createLinearGradient(x0, 0, x1, 0)
  base.addColorStop(0, border); base.addColorStop(stop(x0 + fade), accent); base.addColorStop(0.5, warm); base.addColorStop(stop(x1 - fade), accent); base.addColorStop(1, border)
  const glow = ctx.createLinearGradient(x0, 0, x1, 0)
  glow.addColorStop(0, clear); glow.addColorStop(stop(x0 + fade), accent); glow.addColorStop(0.5, warm); glow.addColorStop(stop(x1 - fade), accent); glow.addColorStop(1, clear)
  const amp = (0.6 + motion.loud * 7.5) * Math.min(1, height / 48)
  const path = () => {
    ctx.beginPath()
    for (let x = x0; x <= x1; x += 2) {
      const u = (x - x0) / span
      const envelope = Math.exp(-((u - motion.center) ** 2) / (2 * 0.13 ** 2))
      const edge = Math.min(1, Math.max(0, (x - x0) / fade), Math.max(0, (x1 - x) / fade)) ** 2
      const ripple = Math.sin(x / 9.5 - motion.phase * 2.4) * 0.6 + Math.sin(x / 15 + motion.phase * 1.3) * 0.3 + Math.sin(x / 31 - motion.phase * 0.7) * 0.25
      const y = mid - ripple * envelope * edge * amp
      if (x > x0) ctx.lineTo(x, y)
      else ctx.moveTo(x, y)
    }
  }
  ctx.lineCap = 'butt'
  ctx.lineJoin = 'round'
  path(); ctx.strokeStyle = base; ctx.lineWidth = 1; ctx.stroke()
  path(); ctx.strokeStyle = glow; ctx.lineWidth = 2; ctx.shadowColor = accent; ctx.shadowBlur = 4 + motion.loud * 6; ctx.stroke()
  ctx.shadowBlur = 0
}
