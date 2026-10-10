// The trunk of the pocket-fold elephant, drawn as one shape along a center
// line. At rest it is the logo's trunk: a column 99 wide from inside the head
// to the foot line, its front foot corner rounded wide and its back corner
// tight. A curl lengthens and tapers it, swings it out and up in front of the
// face, and hooks the tip back over into a J, as a raised trunk does. The tip
// rounds as it curls. The part inside the head never bends.
const TOP = [476.5, 300]
const REST_LENGTH = 159
const WIDTH = 99
const HIDDEN = 38
const STEPS = 32
const BACK_RADIUS = 10
const FRONT_RADIUS = 36

// The base bend lifts the trunk out and up. The hook turns the tip back over.
// A curl comes in two steps, so the trunk is hooked at every frame: the hanging
// trunk thins and its tip curls up first, then the hooked trunk rises and
// lengthens into the J. HOOK_SHARE is the part of a curl the first step takes.
const HOOK_SHARE = 0.4
export const CURL = { length: 140, tipWidth: 44, lift: 140, hook: 110 }
// The sway the stride gives the trunk, in degrees at the tip.
export const SWAY_DEG = 7
// Radians in one degree, for the mark's other turns.
export const DEGREE = Math.PI / 180

const fixed = (n) => String(Math.round(n * 10) / 10)
const point = ([x, y]) => `${fixed(x)} ${fixed(y)}`

// curl runs from 0 at rest to 1 at the full J. sway runs from -1 to 1.
export function trunkPath(curl = 0, sway = 0) {
  const hooked = Math.min(1, curl / HOOK_SHARE)
  const raised = Math.max(0, (curl - HOOK_SHARE) / (1 - HOOK_SHARE))
  const length = REST_LENGTH + CURL.length * raised
  const bend = length - HIDDEN
  const reach = (s) => Math.max(0, (s - HIDDEN) / bend)
  const angle = (s) => {
    const v = reach(s)
    const lift = CURL.lift * Math.sin(Math.PI / 2 * Math.min(1, v / 0.4))
    const hook = CURL.hook * v ** (2 + 2 * raised) * hooked
    return (lift * raised + hook + SWAY_DEG * sway * v) * Math.PI / 180
  }
  const width = (s) => WIDTH - (WIDTH - CURL.tipWidth) * hooked * reach(s)
  const ds = length / STEPS
  let [x, y] = TOP
  const spine = [{ s: 0, x, y, a: 0, w: WIDTH }]
  for (let i = 1; i <= STEPS; i++) {
    const a = angle((i - 0.5) * ds)
    x += Math.sin(a) * ds
    y += Math.cos(a) * ds
    spine.push({ s: i * ds, x, y, a: angle(i * ds), w: width(i * ds) })
  }
  // A point on the spine between two steps, where a foot corner starts.
  const at = (s) => {
    const p = spine[Math.min(STEPS - 1, Math.floor(s / ds))]
    const a = angle((p.s + s) / 2)
    return { s, x: p.x + Math.sin(a) * (s - p.s), y: p.y + Math.cos(a) * (s - p.s), a: angle(s), w: width(s) }
  }
  // The front edge sits on the normal's positive side, the back edge on its negative side.
  const side = (p, k) => [p.x + Math.cos(p.a) * k, p.y - Math.sin(p.a) * k]
  const end = spine[STEPS]
  const half = end.w / 2
  const back = BACK_RADIUS + (half - BACK_RADIUS) * hooked
  const front = FRONT_RADIUS + (half - FRONT_RADIUS) * hooked
  const backCorner = at(length - back)
  const frontCorner = at(length - front)
  // Every outline has the same commands, so an SVG animation can morph one
  // curl into another: a spine point past a foot corner stands on that corner.
  const backEdge = spine.map((p) => p.s < length - back - 0.01 ? side(p, -p.w / 2) : side(backCorner, -backCorner.w / 2))
  const frontEdge = spine.map((p) => p.s < length - front - 0.01 ? side(p, p.w / 2) : side(frontCorner, frontCorner.w / 2)).reverse()
  return [
    `M${point(backEdge[0])}`,
    ...backEdge.slice(1).map((p) => `L${point(p)}`),
    `L${point(side(backCorner, -backCorner.w / 2))}`,
    `A${fixed(back)} ${fixed(back)} 0 0 0 ${point(side(end, back - half))}`,
    `L${point(side(end, half - front))}`,
    `A${fixed(front)} ${fixed(front)} 0 0 0 ${point(side(frontCorner, frontCorner.w / 2))}`,
    ...frontEdge.map((p) => `L${point(p)}`),
    'Z',
  ].join(' ')
}
