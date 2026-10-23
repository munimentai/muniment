import { expect, it, vi } from 'vitest'
import { featureFlags } from '../feature-flags.js'
import { settingsIndex, settingsNav, settingsSections, settingsTarget } from './settings-sections.js'

vi.mock('../feature-flags.js', () => ({ featureFlags: { cloud: false, companyRecord: false } }))

it.each([[false, false], [false, true], [true, false], [true, true]])('keeps eight pages and puts each part on one page with cloud=%s and companyRecord=%s', (cloud, companyRecord) => {
  Object.assign(featureFlags, { cloud, companyRecord })
  expect(settingsSections().map(([id]) => id)).toEqual(['account', 'models', 'tools', 'machines', 'permissions', 'data', 'voice', 'general'])
  const parts = settingsIndex().map(([, part]) => part)
  expect(new Set(parts).size).toBe(parts.length)
  expect(parts.includes('cloud')).toBe(cloud)
  expect(parts.includes('companies')).toBe(companyRecord)
  // A paired runtime refuses the cloud account, so its parts leave the search.
  expect(settingsIndex({ remote: true }).some(([, part]) => part === 'cloud')).toBe(false)
})

it('maps every older page id to the page and part that hold it', () => {
  expect(settingsTarget('providers')).toEqual(['models', 'connections'])
  expect(settingsTarget('decisions')).toEqual(['models', 'roles'])
  expect(settingsTarget('models')).toEqual(['models', null])
  expect(settingsTarget('extend')).toEqual(['tools', 'mcp'])
  expect(settingsTarget('skills')).toEqual(['tools', 'skill'])
  expect(settingsTarget('search')).toEqual(['tools', 'search'])
  expect(settingsTarget('runtimes')).toEqual(['machines', null])
  expect(settingsTarget('profile')).toEqual(['account', 'profile'])
  expect(settingsTarget('memory')).toEqual(['data', 'memory'])
  expect(settingsTarget('context')).toEqual(['data', 'context'])
  expect(settingsTarget('home')).toEqual(['data', 'storage'])
  expect(settingsTarget('appearance')).toEqual(['general', 'appearance'])
  expect(settingsTarget('voice')).toEqual(['voice', null])
  expect(settingsTarget('time')).toEqual(['general', 'time'])
  expect(settingsTarget('motion')).toEqual(['general', 'appearance'])
  expect(settingsTarget('microphone')).toEqual(['voice', 'microphone'])
  expect(settingsTarget('notifications')).toEqual(['general', 'notifications'])
  expect(settingsTarget('this-computer')).toEqual(['machines', 'this-computer'])
})

it('narrows the sidebar to the pages and parts a search matches', () => {
  Object.assign(featureFlags, { cloud: false, companyRecord: false })
  expect(settingsNav('').map(([id, , , parts]) => [id, parts])).toEqual(settingsSections().map(([id]) => [id, []]))
  expect(settingsNav('compaction').map(([id, , , parts]) => [id, parts])).toEqual([['data', [['context', 'Context']]]])
  expect(settingsNav('network').map(([id]) => id)).toEqual(['machines'])
  // A page's own label matches with no part under it.
  expect(settingsNav('gener').map(([id, , , parts]) => [id, parts])).toEqual([['general', []]])
  expect(settingsNav('notifications').map(([id, , , parts]) => [id, parts])).toEqual([['general', [['notifications', 'Notifications']]]])
  expect(settingsNav('dictation').map(([id]) => id)).toEqual(['voice'])
  expect(settingsNav('clock').map(([id, , icon, parts]) => [id, icon, parts])).toEqual([['general', 'sliders-horizontal', [['time', 'Time']]]])
  expect(settingsNav('voice').map(([id, , icon]) => [id, icon])).toEqual([['voice', 'mic']])
  expect(settingsNav('no such setting')).toEqual([])
})
