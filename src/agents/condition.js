// The condition builder's view of an `mcp` trigger condition. The runtime
// evaluates the condition against each tool result. The builder edits a tree
// of groups and checks and writes it back as the same JSON.

export const OPERATORS = [
  ['eq', 'equals'],
  ['ne', 'does not equal'],
  ['gt', 'is greater than'],
  ['gte', 'is at least'],
  ['lt', 'is less than'],
  ['lte', 'is at most'],
  ['contains', 'contains'],
  ['nonempty', 'is not empty'],
  ['exists', 'exists'],
  ['changed', 'changed since the last check'],
]

// Operators that compare with a value the user types.
const VALUED = new Set(['eq', 'ne', 'gt', 'gte', 'lt', 'lte', 'contains'])
export const takesValue = (op) => VALUED.has(op)

// A typed value reads as JSON when it parses, so `3` is a number and `true`
// a boolean, and as text otherwise.
export function parseValue(text) {
  const trimmed = String(text ?? '').trim()
  if (!trimmed) return ''
  try { return JSON.parse(trimmed) } catch { return String(text) }
}

// The text that reads back as `value`.
export function valueText(value) {
  if (value === undefined) return ''
  if (typeof value === 'string') {
    try { JSON.parse(value) } catch { return value }
    return JSON.stringify(value)
  }
  return JSON.stringify(value)
}

export function newCheck() {
  return { type: 'check', path: '', op: 'nonempty', value: '' }
}

export function newGroup(join = 'all') {
  return { type: 'group', join, items: [newCheck()] }
}

// The tree for a condition. A lone check sits in an `all` group, so the
// builder always edits a group.
export function conditionToTree(condition, nested = false) {
  if (condition && typeof condition === 'object') {
    for (const join of ['all', 'any']) {
      if (Array.isArray(condition[join])) {
        return { type: 'group', join, items: condition[join].map((item) => conditionToTree(item, true)) }
      }
    }
    if ('op' in condition) {
      const check = { type: 'check', path: condition.path ?? '', op: condition.op, value: valueText(condition.value) }
      return nested ? check : { type: 'group', join: 'all', items: [check] }
    }
  }
  return nested ? newCheck() : newGroup()
}

export function treeToCondition(node) {
  if (node.type === 'group') return { [node.join]: node.items.map(treeToCondition) }
  const check = { path: node.path.trim(), op: node.op }
  if (takesValue(node.op)) check.value = parseValue(node.value)
  return check
}

// A short reading of a condition, such as `/total is greater than 0`.
export function conditionLabel(condition) {
  if (!condition || typeof condition !== 'object') return ''
  for (const join of ['all', 'any']) {
    if (Array.isArray(condition[join])) {
      const parts = condition[join].map(conditionLabel).filter(Boolean)
      return parts.length > 1 ? `${join === 'all' ? 'all of' : 'any of'} (${parts.join(', ')})` : parts[0] ?? ''
    }
  }
  const words = OPERATORS.find(([op]) => op === condition.op)?.[1] ?? condition.op
  return [condition.path || '/', words, takesValue(condition.op) ? valueText(condition.value) : ''].filter(Boolean).join(' ')
}
