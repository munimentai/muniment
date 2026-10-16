// A model's family and generation, read from its id alone. `claude-opus-5-5`
// is Claude Opus at 5.5, `gpt-6.1-sol` is GPT Sol at 6.1 and `qwen3:8b` is Qwen
// at 3. Sizes, snapshot dates, tags and words such as `latest` or `preview`
// name no generation, so they leave the family unchanged.

const QUALIFIERS = new Set(['latest', 'preview', 'exp', 'experimental', 'stable', 'beta', 'alpha'])
const SIZE = /^(?:\d+(?:\.\d+)?|[a-z]\d+|\d+x\d+)[bmk]$/
const DATE = /^\d{4,}$/
const NUMBER = /^\d+(?:\.\d+)*$/
const JOINED = /^([a-z]*)(\d+(?:\.\d+)*)([a-z]*)$/

// { family, version }: the version is a list of numbers, or null when the id
// names none.
export function modelGeneration(id) {
  const base = String(id ?? '').toLowerCase().split('/').pop().split(':')[0]
  const tokens = base.split(/[-_\s]+/).filter(Boolean)
  // A snapshot date such as `0309`, `20251001` or `04-2026` follows the name.
  for (let index = tokens.length - 1; index >= 0; index--) {
    if (DATE.test(tokens[index])) {
      tokens.splice(index, 1)
      if (/^\d{2}$/.test(tokens[index - 1] ?? '')) tokens.splice(--index, 1)
    }
  }
  // The first number is the version, and numbers right after it extend it,
  // as `opus-4-5` does. A joined token such as `qwen3` or `4o` splits.
  const family = []
  let version = null
  let ended = false
  for (const token of tokens) {
    const joined = token.match(JOINED)
    if (QUALIFIERS.has(token) || SIZE.test(token)) ended = !!version
    else if (version && !ended && NUMBER.test(token)) version.push(...token.split('.'))
    else if (!version && joined) {
      const [, prefix, number, suffix] = joined
      version = number.split('.')
      family.push(...[prefix, suffix].filter(Boolean))
      ended = !!suffix
    } else {
      ended = !!version
      family.push(token)
    }
  }
  return { family: family.join('-'), version: version?.map(Number) ?? null }
}

// Versions compare part by part. A part after the first reads as a decimal
// fraction, so 4.20 comes before 4.7.
function compare(a, b) {
  for (let index = 0; index < Math.max(a.length, b.length); index++) {
    const left = index === 0 ? Number(a[0] ?? 0) : Number(`0.${a[index] ?? 0}`)
    const right = index === 0 ? Number(b[0] ?? 0) : Number(`0.${b[index] ?? 0}`)
    if (left !== right) return left - right
  }
  return 0
}

// The ids of the newest generation of each family. An id that names no
// version cannot be placed, so it counts as current.
export function currentGeneration(ids) {
  const read = ids.map((id) => [id, modelGeneration(id)])
  const newest = new Map()
  for (const [, { family, version }] of read) {
    if (version && (!newest.has(family) || compare(version, newest.get(family)) > 0)) newest.set(family, version)
  }
  return new Set(read.filter(([, { family, version }]) => !version || compare(version, newest.get(family)) === 0).map(([id]) => id))
}
