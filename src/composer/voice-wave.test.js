import { describe, expect, it } from 'vitest'
import { VOICE_WAVE_WARM, createVoiceWaveMotion, drawVoiceWave, parseColor } from './voice-wave.js'
import { recordingContext } from './voice-wave-fixtures.js'

const run = (motion, seconds, target) => { for (let i = 0; i < Math.round(seconds * 100); i += 1) motion.step(0.01, target) }

describe('voice wave motion', () => {
  it('reads hex and rgb colors', () => {
    expect(parseColor('#58B39F')).toEqual([0x58, 0xb3, 0x9f])
    expect(parseColor('#abc')).toEqual([0xaa, 0xbb, 0xcc])
    expect(parseColor('rgb(47, 51, 50)')).toEqual([47, 51, 50])
    expect(parseColor('rgba(1, 2, 3, 0.5)')).toEqual([1, 2, 3])
    expect(parseColor('oklch(0.7 0.1 180)')).toBeNull()
  })

  it('rises quickly and falls slowly with the loudness', () => {
    const motion = createVoiceWaveMotion({ random: () => 0.5 })
    // One attack time reaches 1 - 1/e of the step, one release time falls by the same share.
    run(motion, 0.07, 1)
    expect(motion.loud).toBeCloseTo(1 - Math.exp(-1), 2)
    run(motion, 0.5, 1)
    const before = motion.loud
    run(motion, 0.38, 0)
    expect(motion.loud / before).toBeCloseTo(Math.exp(-1), 2)
    run(motion, 4, 0)
    expect(motion.settled()).toBe(true)
  })

  it('warms with the voice and cools back in silence', () => {
    const motion = createVoiceWaveMotion({ random: () => 0.5 })
    run(motion, 3, 1)
    expect(motion.blend).toBeGreaterThan(0.95)
    run(motion, 6, 0)
    expect(motion.blend).toBeLessThan(0.02)
  })

  it('moves the ripples faster while loud and drifts their center only while speaking', () => {
    const quiet = createVoiceWaveMotion({ random: () => 0 })
    run(quiet, 1, 0)
    expect(quiet.phase).toBeCloseTo(0.9, 1)
    expect(quiet.center).toBeCloseTo(0.5, 5)
    const loud = createVoiceWaveMotion({ random: () => 0 })
    run(loud, 1, 1)
    expect(loud.phase).toBeGreaterThan(2)
    expect(loud.centerTarget).toBe(0.3)
    expect(loud.center).toBeLessThan(0.5)
  })
})

describe('voice wave drawing', () => {
  it('runs from the border color into the accent and warms the middle', () => {
    const ctx = recordingContext()
    const motion = createVoiceWaveMotion({ random: () => 0.5 })
    run(motion, 3, 1)
    drawVoiceWave(ctx, { width: 760, height: 48, radius: 18, border: 'rgb(47, 51, 50)', accent: '#58B39F', motion })
    const [base, glow] = ctx.gradients
    expect(base.stops[0]).toEqual([0, 'rgb(47, 51, 50)'])
    expect(base.stops.at(-1)).toEqual([1, 'rgb(47, 51, 50)'])
    expect(base.stops[1][1]).toBe('#58B39F')
    const [r, g, b] = parseColor(base.stops[2][1])
    const [wr] = parseColor(VOICE_WAVE_WARM)
    expect(r).toBeGreaterThan(0x58 + (wr - 0x58) * 0.5)
    expect(base.stops[1][0]).toBeCloseTo(Math.min(140, 724 * 0.18) / 724, 5)
    expect(glow.stops[0][1]).toBe('rgba(88, 179, 159, 0)')
    expect(ctx.strokes).toEqual([
      { style: base, width: 1, shadowColor: undefined, shadowBlur: undefined },
      { style: glow, width: 2, shadowColor: '#58B39F', shadowBlur: 4 + motion.loud * 6 },
    ])
    expect(g + b).toBeGreaterThan(0)
  })

  it('keeps the plain accent in the middle in silence', () => {
    const ctx = recordingContext()
    drawVoiceWave(ctx, { width: 400, height: 48, radius: 18, border: '#000000', accent: '#A894F2', motion: createVoiceWaveMotion() })
    expect(ctx.gradients[0].stops[2][1]).toBe('rgb(168, 148, 242)')
  })
})
