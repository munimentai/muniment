// A 2D context that records the gradient stops, strokes and glow of each draw.
export function recordingContext() {
  const ctx = { gradients: [], strokes: [], cleared: 0, points: 0 }
  Object.assign(ctx, {
    clearRect: () => { ctx.cleared += 1 },
    setTransform: () => {},
    createLinearGradient: () => { const gradient = { stops: [], addColorStop: (offset, color) => gradient.stops.push([offset, color]) }; ctx.gradients.push(gradient); return gradient },
    beginPath: () => {},
    moveTo: () => { ctx.points += 1 },
    lineTo: () => { ctx.points += 1 },
    stroke: () => ctx.strokes.push({ style: ctx.strokeStyle, width: ctx.lineWidth, shadowColor: ctx.shadowColor, shadowBlur: ctx.shadowBlur }),
  })
  return ctx
}
