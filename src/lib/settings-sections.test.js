import { expect, it, vi } from 'vitest'
import { featureFlags } from '../feature-flags.js'
import { settingsGroups } from './settings-sections.js'

vi.mock('../feature-flags.js', () => ({ featureFlags: { cloud: false, companyRecord: false } }))

const pages = (options) => settingsGroups(options).map(([group, entries]) => [group, entries.map(([id]) => id)])

it.each([[false, false], [false, true], [true, false], [true, true]])('closes Extend with Machines and Personal with Storage with cloud=%s and companyRecord=%s', (cloud, companyRecord) => {
  Object.assign(featureFlags, { cloud, companyRecord })
  expect(pages()).toEqual([
    ['Models', ['providers', 'decisions']],
    ['Extend', ['mcp', 'skills', 'plugins', 'runtimes']],
    ['Preferences', ['appearance', 'context', 'permissions', 'voice', 'search', 'language']],
    ['Personal', ['profile', 'memory', ...(cloud ? ['account'] : []), 'home']],
    // Workspace holds only Companies, so it shows only with the company record.
    ...(companyRecord ? [['Workspace', ['companies']]] : []),
  ])
  expect(pages({ remote: true }).find(([group]) => group === 'Personal')[1]).toEqual(['profile', 'memory', 'home'])
  expect(settingsGroups()[1][1].at(-1)).toEqual(['runtimes', 'Machines', 'network'])
})
