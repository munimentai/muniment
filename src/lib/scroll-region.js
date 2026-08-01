export function scrollRegionOverflows({ scrollWidth, clientWidth }) {
  return Number.isFinite(scrollWidth) && Number.isFinite(clientWidth) && scrollWidth > clientWidth
}
