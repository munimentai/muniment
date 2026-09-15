import milledRingSvg from '../../src-tauri/icons/muniment-milled-ring.svg?raw'

// Keep every in-app mark on the same owner-supplied geometry used to generate
// the native application icons.
const pathMatch = milledRingSvg.match(/<path d="([^"]+)"/)

export const MILLED_RING_PATH = pathMatch[1]

export function ringPath() {
  return MILLED_RING_PATH
}

const CENTER = 24
const BASE_RADIUS = 20.2
const MILL_DEPTH = 1.15
const TOOTH_COUNT = 22
const REDUCTION_WIDTH = 5
const REDUCTION_SAMPLES = TOOTH_COUNT * 12

function sampledEdge(radiusOffset, reverse = false) {
  const points = []
  for (let index = 0; index < REDUCTION_SAMPLES; index += 1) {
    const sample = reverse ? REDUCTION_SAMPLES - index : index
    const angle = sample / REDUCTION_SAMPLES * Math.PI * 2
    const radius = BASE_RADIUS + radiusOffset + MILL_DEPTH * Math.sin(TOOTH_COUNT * angle)
    points.push(`${index ? 'L' : 'M'}${(CENTER + radius * Math.cos(angle)).toFixed(3)},${(CENTER + radius * Math.sin(angle)).toFixed(3)}`)
  }
  return `${points.join(' ')} Z`
}

// At chip scale the monoline loses its teeth. Two sampled, closed edges keep
// the same 22-tooth wave legible as a solid ring without changing the owner
// geometry used by rest-state marks.
export function solidMilledRingPath() {
  return `${sampledEdge(REDUCTION_WIDTH / 2)} ${sampledEdge(-REDUCTION_WIDTH / 2, true)}`
}

// The seal band, measured from the owner's seal-light-1024.png in the 48-unit
// viewBox: 22 trapezoid teeth, listed as [degrees from the valley axis, radius],
// a band 3.98 wide at its centre radius 20.43, first valley axis at 12.27
// degrees. `ampMul` flexes the milling depth, `scale` the whole ring, and
// `width` sets the band width. This is the geometry of the mark in flight, and
// docs/design-reference/ring/muniment-ring-pulse-spin.html is its source.
const TOOTH = {
  outer: [[0, 20.967], [2.48, 22.556], [5.45, 23.906], [10.91, 23.906], [13.88, 22.556]],
  inner: [[3.6, 17.170], [6.5, 18.609], [8.18, 19.945], [9.86, 18.609], [12.76, 17.170]],
}
const SEAL_RADIUS = 20.43
export const SEAL_BAND_WIDTH = 3.98
const TOOTH_STEP = 360 / TOOTH_COUNT
const VALLEY = 12.27

function toothPoints(list, ampMul, scale, width, sign) {
  const points = []
  for (let tooth = 0; tooth < TOOTH_COUNT; tooth += 1) {
    for (const [degrees, radius0] of list) {
      const angle = (VALLEY + tooth * TOOTH_STEP + degrees) * Math.PI / 180
      const radius = (SEAL_RADIUS + (radius0 - SEAL_RADIUS) * ampMul + sign * (width - SEAL_BAND_WIDTH * ampMul) / 2) * scale
      points.push(`${(CENTER + radius * Math.cos(angle)).toFixed(2)},${(CENTER + radius * Math.sin(angle)).toFixed(2)}`)
    }
  }
  return points
}

// The filled band: outer edge, then the inner edge reversed, for evenodd.
export function sealBandPath(ampMul = 1, scale = 1, width = SEAL_BAND_WIDTH) {
  return `M${toothPoints(TOOTH.outer, ampMul, scale, width, 1).join(' L')} Z M${toothPoints(TOOTH.inner, ampMul, scale, width, -1).reverse().join(' L')} Z`
}

// The band's centreline, the path the trace accent runs.
export function sealTracePath(ampMul = 1, scale = 1) {
  const mid = [[0, (20.967 + 17.170) / 2], [8.18, (23.906 + 19.945) / 2]]
  return `M${toothPoints(mid, ampMul, scale, SEAL_BAND_WIDTH, 0).join(' L')} Z`
}
