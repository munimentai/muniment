import fs from 'node:fs'
import path from 'node:path'
import { render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, expect, it, vi } from 'vitest'
import { featureFlags } from '../feature-flags.js'
import Settings from './Settings.svelte'
import { settingsIndex, settingsNav, settingsSections } from './settings-sections.js'

vi.mock('../feature-flags.js', () => ({ featureFlags: { cloud: false, companyRecord: false } }))

// Each page renders with a runtime that answers core's operations from the
// protocol fixtures and its own commands with this computer alone, so the
// page's fixed headings and rows show.
const thisComputer = { appVersion: '0.4.0', deviceName: 'This Mac', selected: 'local', runtimes: [{ id: 'local', name: 'This Mac', kind: 'local', status: 'connected' }] }
const fixtures = path.join(process.cwd(), 'core/protocol-fixtures/muniment.attach/1')
function answer(operation) {
  const file = path.join(fixtures, `response-${operation.replace(/[._]/g, '-')}.json`)
  return fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')).body : null
}
const tauri = { invoke: vi.fn(async (command, args) => command === 'runtime_list' ? thisComputer : command === 'agent_request' ? answer(args?.operation ?? '') : null) }

afterEach(() => { document.body.innerHTML = '' })

// The words a heading or a row title shows, without its count, meta or info control.
const own = (node) => [...node.childNodes].filter((child) => child.nodeType === 3).map((child) => child.textContent).join('').trim()

async function labels(page) {
  const view = render(Settings, { tauri, section: page, onclose: () => {} })
  await waitFor(() => expect(document.body.textContent).not.toContain('Loading settings'), { timeout: 10_000 })
  await new Promise((resolve) => setTimeout(resolve, 200))
  const found = [
    ...[...document.querySelectorAll('.page-title.part-title')].map((node) => node.textContent.trim()),
    ...[...document.querySelectorAll('.group-heading')].map(own),
    ...[...document.querySelectorAll('.row-text > strong')].map(own),
  ].filter(Boolean)
  view.unmount()
  return [...new Set(found)]
}

it.each([[false, false], [true, true]])('finds every heading and row title on every page with cloud=%s and companyRecord=%s', async (cloud, companyRecord) => {
  Object.assign(featureFlags, { cloud, companyRecord })
  const missing = []
  // Each page, and each part that opens its own tab or block.
  const targets = [...settingsSections().map(([page]) => [page, page]), ...settingsIndex().map(([page, part]) => [page, part])]
  for (const [page, section] of targets) {
    const shown = await labels(section)
    expect(shown.length, section).toBeGreaterThan(0)
    for (const label of shown) {
      if (!settingsNav(label).some(([id]) => id === page)) missing.push(`${page}: ${label}`)
    }
  }
  expect(missing).toEqual([])
}, 120_000)

it.each([
  ['motion', 'general'], ['animations', 'general'], ['reduce motion', 'general'], ['dark mode', 'general'], ['high contrast', 'general'],
  ['bigger text', 'general'], ['spacing', 'general'], ['timezone', 'general'], ['12 hour', 'general'], ['military time', 'general'],
  ['alerts', 'general'], ['locale', 'general'], ['push to talk', 'voice'], ['speech to text', 'voice'], ['mic', 'voice'],
  ['api key', 'models'], ['ollama', 'models'], ['integrations', 'tools'], ['docker', 'machines'], ['protected paths', 'permissions'],
  ['zip', 'data'], ['sign out', 'account'],
])('finds %s on %s', (query, page) => {
  Object.assign(featureFlags, { cloud: true, companyRecord: false })
  expect(settingsNav(query).map(([id]) => id)).toContain(page)
})

it('shows Voice on its own page and leaves General with appearance, language, time, notifications and about', async () => {
  Object.assign(featureFlags, { cloud: false, companyRecord: false })
  const voice = await labels('voice')
  expect(voice).toEqual(expect.arrayContaining(['Shortcut', 'Microphone', 'Speech model', 'Transcription', 'Dictation shortcut', 'Custom words']))
  render(Settings, { tauri, section: 'voice', onclose: () => {} })
  expect(await screen.findByRole('heading', { name: 'Voice', level: 3 })).toBeInTheDocument()
  document.body.innerHTML = ''
  const general = await labels('general')
  expect(general.slice(0, 5)).toEqual(['Appearance', 'Language', 'Time', 'Notifications', 'About'])
  expect(general).toEqual(expect.arrayContaining(['Type', 'Display', 'Motion', 'Thinking indicator', 'Time zone', 'Clock']))
  expect(general).not.toContain('Dictation shortcut')
  expect(general).not.toContain('Microphone')
})
