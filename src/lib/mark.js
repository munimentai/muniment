import milledRingSvg from '../../src-tauri/icons/muniment-milled-ring.svg?raw'

// Keep every in-app mark on the same owner-supplied geometry used to generate
// the native application icons.
const pathMatch = milledRingSvg.match(/<path d="([^"]+)"/)

export const MILLED_RING_PATH = pathMatch[1]

export function ringPath() {
  return MILLED_RING_PATH
}

const CENTER = 24
const BASE_RADIUS = 16.5
const MILL_DEPTH = 1.6
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
