import RECENCY from './model-recency.json'

// A model released this long before its provider's newest one is past.
const STALE_DAYS = 365
const DAY_MS = 86400000

const day = (date) => Date.parse(date.length === 7 ? `${date}-01` : date)

// The models a provider has moved past, among the ids it serves: the ones the
// snapshot marks deprecated, and the ones released more than a year before the
// provider's newest model. The snapshot comes from models.dev by
// scripts/refresh-model-recency.py. An id or a provider the snapshot does not
// know, such as a local or custom endpoint, is never past.
export function pastModels(catalogId, ids) {
  const known = RECENCY[catalogId]
  if (!known) return new Set()
  const latest = Math.max(...ids.filter((id) => known[id]).map((id) => day(known[id][1])))
  return new Set(ids.filter((id) => known[id] && (!!known[id][2] || latest - day(known[id][1]) > STALE_DAYS * DAY_MS)))
}

// Newest first: a model the snapshot does not know comes first, as newer than
// the snapshot, then the rest by release date. A provider the snapshot does not
// know keeps its own order.
export function newestFirst(catalogId, ids) {
  const known = RECENCY[catalogId]
  if (!known) return [...ids]
  const age = (id) => known[id] ? -day(known[id][1]) : -Infinity
  return ids.map((id, index) => [id, index]).sort(([a, i], [b, j]) => age(a) - age(b) || i - j).map(([id]) => id)
}
