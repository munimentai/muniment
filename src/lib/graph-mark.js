// Vertices from brand/logo/ring-graph.svg. Native and UI marks share this geometry.
export const GRAPH_POINTS = [[42.9,28.11],[45.61,29.69],[46.48,31.19],[45.7,33.29],[44.07,33.85],[40.97,33.27],[43.14,35.56],[43.54,37.23],[42.2,39.03],[40.48,39.11],[37.68,37.68],[39.11,40.47],[39.03,42.2],[37.23,43.54],[35.56,43.14],[33.27,40.97],[33.85,44.07],[33.29,45.69],[31.19,46.48],[29.7,45.62],[28.11,42.9],[27.81,46.04],[26.8,47.43],[24.56,47.59],[23.38,46.35],[22.63,43.28],[21.44,46.21],[20.09,47.27],[17.89,46.8],[17.1,45.27],[17.24,42.13],[15.29,44.58],[13.69,45.23],[11.72,44.15],[11.39,42.47],[12.42,39.49],[9.83,41.31],[8.13,41.46],[6.54,39.87],[6.71,38.16],[8.53,35.59],[5.55,36.6],[3.85,36.28],[2.77,34.32],[3.42,32.71],[5.89,30.76],[2.74,30.9],[1.2,30.11],[0.73,27.92],[1.79,26.56],[4.72,25.38],[1.65,24.63],[0.41,23.44],[0.57,21.2],[1.97,20.2],[5.1,19.89],[2.39,18.31],[1.52,16.81],[2.3,14.71],[3.93,14.15],[7.03,14.73],[4.86,12.44],[4.46,10.77],[5.8,8.97],[7.52,8.89],[10.32,10.32],[8.89,7.53],[8.97,5.8],[10.77,4.46],[12.44,4.86],[14.73,7.03],[14.15,3.93],[14.71,2.31],[16.81,1.52],[18.3,2.38],[19.89,5.1],[20.19,1.96],[21.2,0.57],[23.44,0.41],[24.62,1.65],[25.37,4.72],[26.56,1.79],[27.91,0.73],[30.11,1.2],[30.9,2.73],[30.76,5.87],[32.71,3.42],[34.31,2.77],[36.28,3.85],[36.61,5.53],[35.58,8.51],[38.17,6.69],[39.87,6.54],[41.46,8.13],[41.29,9.84],[39.47,12.41],[42.45,11.4],[44.15,11.72],[45.23,13.68],[44.58,15.29],[42.11,17.24],[45.26,17.1],[46.8,17.89],[47.27,20.08],[46.21,21.44],[43.28,22.62],[46.35,23.37],[47.59,24.56],[47.43,26.8],[46.03,27.8]]

export function graphVariant(size) {
  if (size <= 24) return { count: 22, jump: 3, width: 0.7, outlineWidth: 0.75 }
  if (size < 48) return { count: 33, jump: 5, width: 0.46, outlineWidth: 0.48 }
  if (size < 96) return { count: 55, jump: 8, width: 0.23, outlineWidth: 0.26 }
  return { count: 110, jump: 17, width: 0.08, outlineWidth: 0 }
}

export function graphPoint([x, y], pose = {}) {
  const { ampMul = 1, scale = 1 } = pose
  const dx = x - 24; const dy = y - 24
  const r = Math.hypot(dx, dy)
  const innerWeight = Math.max(0, Math.min(1, (24 - r) / 5))
  const inward = 4.5 * Math.max(0, (ampMul - 1) / 0.6) * innerWeight
  const radius = (20.43 + (r - 20.43) * ampMul - inward) * scale
  return [24 + dx / r * radius, 24 + dy / r * radius]
}

const pair = (point) => point.map((n) => Number(n.toFixed(4))).join(',')
export function graphPaths(size, pose = {}) {
  const variant = graphVariant(size)
  const points = GRAPH_POINTS.map((point) => graphPoint(point, pose))
  const selected = Array.from({ length: variant.count }, (_, i) => points[Math.floor(i * points.length / variant.count)])
  const offsets = variant.count === 110 ? [7, 17] : [1, variant.jump]
  const edges = selected.flatMap((point, i) => offsets.map((offset) => `M${pair(point)} L${pair(selected[(i + offset) % selected.length])}`)).join(' ')
  const outline = `M${points.map(pair).join(' L')} Z`
  return { ...variant, edges, outline, points, width: variant.width * (pose.widthMul ?? 1), outlineWidth: variant.outlineWidth * (pose.widthMul ?? 1) }
}

// Static native assets and browser callback pages share the UI's size reductions.
export function graphSvg(size) {
  const graph = graphPaths(size)
  const outline = graph.outlineWidth ? `<path d="${graph.outline}" stroke-width="${graph.outlineWidth}"/>` : ''
  const dots = size >= 96 ? graph.points.map(([x, y]) => `<circle cx="${x}" cy="${y}" r="0.2" fill="currentColor" stroke="none"/>`).join('') : ''
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 48 48" role="img" aria-label="muniment"><title>muniment</title><g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><path d="${graph.edges}" stroke-width="${graph.width}"/>${outline}${dots}</g></svg>`
}
