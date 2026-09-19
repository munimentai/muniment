import { expect, it } from 'vitest'
import { importTemplate, exportTemplate, exportGrokSetup, importGrokPage, publicTemplateUrl } from './agent-templates.js'
import { parseProfile, serializeProfile } from './profile-fields.js'
it.each(['muniment-v1', 'muniment-v2'])('round trips descriptions, job titles and %s avatars without private state or active execution', style => {
  const original = { id: 'private-id', name: 'Scout', label: 'Researcher', instructions: 'Find sources.', avatar: { style, seed: 'avatar-one' }, projectId: 'private-project', memories: ['private'], schedule: { enabled: true, cadence: 'weekly', weekday: 2, time: '09:30' } }
  const file = exportTemplate(original)
  expect(file).not.toContain('private')
  expect(importTemplate(file)).toEqual({ id: '', name: 'Scout', label: 'Researcher', instructions: 'Find sources.', avatar: original.avatar, template: null, projectId: null, schedule: { ...original.schedule, enabled: false } })
  expect(importTemplate(exportGrokSetup(original))).toEqual(importTemplate(file))
})
it('retains supplied Grok template sections and unknown configuration across edits and exports', () => {
  const original = { botName: 'Scout', label: 'Researcher', description: 'Find sources.', skills: [{ name: 'Source check', instructions: 'Check dates.' }], plugins: ['github'], routines: [{ cron: '0 9 * * *' }], memories: ['Use original sources.'], customField: 'preserve this' }
  const imported = importTemplate(JSON.stringify(original))
  expect(imported.template.original).toEqual(original)
  expect(imported.template.skills).toEqual(original.skills)
  expect(imported.schedule).toBeNull()
  expect(importTemplate(exportTemplate({ ...imported, name: 'New name' })).template).toEqual(imported.template)
  expect(exportGrokSetup(imported)).toContain('Keep routines paused')
})
it('accepts legacy and Markdown templates and validates bounds', () => {
  expect(importTemplate('# Researcher\n\n## Skills\nCheck sources.').instructions).toContain('## Skills\nCheck sources.')
  expect(importTemplate('{"format":"muniment-agent","version":1,"name":"Old","instructions":"Keep this."}').instructions).toBe('Keep this.')
  expect(() => importTemplate('{"format":"muniment-agent","version":99}')).toThrow('version')
  expect(() => importTemplate('# Empty\n')).toThrow()
  expect(() => importTemplate('x'.repeat(131073))).toThrow('128 KB')
})
const url = 'https://x.ai/bot/rfAHsaFrz6xHBMtUpxDi5'
const page = data => `<script>self.__next_f.push(${JSON.stringify([1, `2b:${JSON.stringify({ state: { queries: [{ state: { data } }] } })}\n`])})</script>`
it('reads the observed Grok public page data without running scripts or pretending it is the full package', () => {
  const data = { id: 'rfAHsaFrz6xHBMtUpxDi5', botName: 'Dewey', description: 'Watch mail. Never send.', color: '#1084FE', shape: 'wedge' }
  const draft = importGrokPage({ url, html: page(data) + '<script>throw new Error("must not execute")</script>' })
  expect(draft.name).toBe('Dewey')
  expect(draft.template.coverage).toBe('public-profile')
  expect(draft.template.sourceUrl).toBe(url)
  expect(draft.template.original.shape).toBe('wedge')
  expect(() => importGrokPage({ url, html: page({ ...data, id: 'wrong' }) })).toThrow('could not be read')
  expect(() => importGrokPage({ url, html: '<h1>Not found</h1>' })).toThrow()
})
it('confines URL imports to the official public share path', () => {
  expect(publicTemplateUrl(url + '/dewey?tracking=1')).toBe(url)
  for (const bad of [url.replace('https:', 'http:'), url.replace('x.ai', 'x.ai.evil.test'), url.replace('x.ai', 'user@x.ai'), 'https://x.ai/bot/marketplace']) expect(() => publicTemplateUrl(bad)).toThrow()
})
it('preserves unknown and legacy profile text while editing individual fields', () => {
  const text = '# Profile\n\nLegacy context.\n\n## Name\n\nSam\n\n## Instructions\n\nBe concise.\n\n## Favorites\n\nGreen'
  const parsed = parseProfile(text)
  const saved = serializeProfile({ ...parsed, preferredName: 'Alex' })
  expect(saved).toContain('Legacy context.')
  expect(saved).toContain('## Favorites\n\nGreen')
  expect(parseProfile(saved).preferredName).toBe('Alex')
})
