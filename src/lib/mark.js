// The milled ring at rest — design-spec §1.8 reference geometry:
// r(t) = 16.5 + 1.6·sin(22t) in a 48-unit viewBox, monoline stroke, round caps
// (engine: docs/design-reference/ring/muniment-ring-pulse-spin.html).
export function ringPath(points = 220) {
  const pts = []
  for (let i = 0; i <= points; i++) {
    const t = (2 * Math.PI * i) / points
    const r = 16.5 + 1.6 * Math.sin(22 * t)
    pts.push(`${(24 + r * Math.cos(t)).toFixed(2)},${(24 + r * Math.sin(t)).toFixed(2)}`)
  }
  return `M${pts.join(' L')} Z`
}
