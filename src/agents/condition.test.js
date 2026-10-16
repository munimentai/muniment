import { describe, expect, it } from 'vitest'
import { OPERATORS, conditionLabel, conditionToTree, parseValue, takesValue, treeToCondition, valueText } from './condition.js'

// The condition in core's routine.save fixture.
const fixture = { all: [{ op: 'gt', path: '/total', value: 0 }, { any: [{ op: 'eq', path: '/items/0/region', value: 'west' }, { op: 'changed', path: '/items' }] }] }

describe('condition builder', () => {
  it('reads the fixture condition and writes the same JSON back', () => {
    const tree = conditionToTree(fixture)
    expect(tree).toEqual({ type: 'group', join: 'all', items: [
      { type: 'check', path: '/total', op: 'gt', value: '0' },
      { type: 'group', join: 'any', items: [
        { type: 'check', path: '/items/0/region', op: 'eq', value: 'west' },
        { type: 'check', path: '/items', op: 'changed', value: '' },
      ] },
    ] })
    expect(treeToCondition(tree)).toEqual(fixture)
  })

  it('round trips every operator and value type', () => {
    for (const [op] of OPERATORS) {
      for (const value of [0, 2.5, 'west', '0', true, null, ['a'], { b: 1 }]) {
        const condition = takesValue(op) ? { path: '/x', op, value } : { path: '/x', op }
        expect(treeToCondition(conditionToTree({ any: [condition] }))).toEqual({ any: [condition] })
      }
    }
  })

  it('edits a lone check inside an all group', () => {
    const tree = conditionToTree({ path: '/count', op: 'gte', value: 3 })
    expect(tree.join).toBe('all')
    expect(treeToCondition(tree)).toEqual({ all: [{ path: '/count', op: 'gte', value: 3 }] })
    expect(treeToCondition(conditionToTree(null))).toEqual({ all: [{ path: '', op: 'nonempty' }] })
  })

  it('reads typed values as JSON when they parse and as text otherwise', () => {
    expect(parseValue('3')).toBe(3)
    expect(parseValue('true')).toBe(true)
    expect(parseValue('"3"')).toBe('3')
    expect(parseValue('west coast')).toBe('west coast')
    expect(valueText('3')).toBe('"3"')
    expect(valueText('west')).toBe('west')
    expect(valueText(undefined)).toBe('')
  })

  it('drops the value of a check that compares with nothing', () => {
    const tree = conditionToTree({ all: [{ path: '/items', op: 'eq', value: 1 }] })
    tree.items[0].op = 'changed'
    expect(treeToCondition(tree)).toEqual({ all: [{ path: '/items', op: 'changed' }] })
  })

  it('reads a condition in words', () => {
    expect(conditionLabel(fixture)).toBe('all of (/total is greater than 0, any of (/items/0/region equals west, /items changed since the last check))')
    expect(conditionLabel({ all: [{ path: '', op: 'nonempty' }] })).toBe('/ is not empty')
  })
})
