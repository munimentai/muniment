import RECENCY from './model-recency.json'

// A family whose newest release trails its provider's newest model by more than
// this is a past generation, and none of its models leads the list.
const STALE_DAYS = 365
const DAY_MS = 86400000

const day = (date) => Date.parse(date.length === 7 ? `${date}-01` : date)

// The models that lead a provider's list: the newest release of each family the
// provider still ships, among the ids it serves. The snapshot comes from
// models.dev by scripts/refresh-model-recency.py. An id the snapshot does not
// know is newer than the snapshot, so it leads, and a provider the snapshot does
// not know, such as a local or custom endpoint, leads with every model.
export function leadingModels(catalogId, ids) {
  const known = RECENCY[catalogId]
  if (!known) return new Set(ids)
  const dated = ids.filter((id) => known[id] && !known[id][2])
  const newest = new Map()
  for (const id of dated) {
    const [family, date] = known[id]
    const key = family || id
    if (!newest.has(key) || day(date) > newest.get(key)) newest.set(key, day(date))
  }
  const latest = Math.max(...newest.values())
  return new Set(ids.filter((id) => {
    if (!known[id]) return true
    const [family, date, deprecated] = known[id]
    const top = newest.get(family || id)
    return !deprecated && day(date) === top && latest - top <= STALE_DAYS * DAY_MS
  }))
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
