import { describe, expect, it, vi } from 'vitest'
import { commitBody, commitImport, exportAgent, importPreview, importSource, previewFacts, readTemplateFile } from './import.js'

const preview = {
  agent: { id: '', name: 'Dewey', routines: [{ id: 'import-1', proposed: true, enabled: false }] },
  memories: [{ title: 'Signature', content: 'First name only.' }],
  skills: { matched: [{ id: 'review', name: 'Review' }], proposals: [{ name: 'Tone', instructions: 'Write plainly.' }] },
  tools: { matched: [], setup: [{ name: 'Gmail' }] },
  routine_notes: [{ name: 'On new mail', note: 'Set one up by hand.' }],
  source: 'grok_link',
  coverage: 'public-profile',
}

describe('the import source', () => {
  it('takes the link, the file text or the pasted text the chosen way names', () => {
    expect(importSource({ way: 'link', link: ' https://x.ai/bot/abc ', paste: 'ignored' })).toBe('https://x.ai/bot/abc')
    expect(importSource({ way: 'file', file: '{"name":"A"}' })).toBe('{"name":"A"}')
    expect(importSource({ way: 'paste', paste: '# Dewey\nDrafts replies.' })).toBe('# Dewey\nDrafts replies.')
  })
  it('refuses an empty source, a link that is not https and a source over 128 KB', () => {
    expect(() => importSource({ way: 'link', link: '' })).toThrow('Paste a share link.')
    expect(() => importSource({ way: 'link', link: 'http://x.ai/bot/a' })).toThrow('Paste a link that starts with https://.')
    expect(() => importSource({ way: 'file' })).toThrow('Choose a template file.')
    expect(() => importSource({ way: 'paste', paste: 'x'.repeat(131073) })).toThrow('Choose a template under 128 KB.')
  })
  it('reads a chosen file and refuses a large one', async () => {
    expect(await readTemplateFile({ size: 5, text: async () => 'hello' })).toBe('hello')
    await expect(readTemplateFile({ size: 200000, text: async () => '' })).rejects.toThrow('Choose a template under 128 KB.')
    expect(await readTemplateFile(null)).toBe('')
  })
})

describe('preview and commit', () => {
  it('asks core for the preview and commits the agent with or without its memories', async () => {
    const invoke = vi.fn(async (_command, { operation }) => operation === 'agent.import' ? preview : { agent: { id: 'new' } })
    expect(await importPreview({ invoke }, 'local', 'https://x.ai/bot/abc')).toBe(preview)
    await commitImport({ invoke }, 'box', preview, { memories: false })
    expect(invoke.mock.calls.map(([, payload]) => [payload.runtime, payload.operation, payload.body])).toEqual([
      ['local', 'agent.import', { source: 'https://x.ai/bot/abc' }],
      ['box', 'agent.import_commit', { agent: preview.agent }],
    ])
    expect(commitBody(preview)).toEqual({ agent: preview.agent, memories: preview.memories })
    expect(commitBody({ agent: preview.agent, memories: [] })).toEqual({ agent: preview.agent })
  })
  it('names the source, the profile-only coverage and the routines that arrive off', () => {
    expect(previewFacts(preview)).toEqual({ source: 'Grok share link', coverage: 'The share page holds the public profile only. Skills, memories and routines are not in it.', routines: '1 routine arrives off, as proposals to accept.' })
    expect(previewFacts({ agent: { routines: [] }, source: 'muniment', coverage: 'provided' })).toEqual({ source: 'Muniment agent file', coverage: '', routines: 'No routines.' })
  })
})

it('saves the file core exports under the name core gives it', async () => {
  const invoke = vi.fn(async (command) => command === 'agent_request' ? { format: 'grok_bot', file_name: 'Lead-scout.grok-bot.md', content: '# Lead scout' } : true)
  expect(await exportAgent({ invoke }, { id: 'a1', runtimeId: 'box' }, 'grok_bot')).toBe(true)
  expect(invoke.mock.calls).toEqual([
    ['agent_request', { runtime: 'box', operation: 'agent.export', body: { id: 'a1', format: 'grok_bot' } }],
    ['agent_export_template', { content: '# Lead scout', fileName: 'Lead-scout.grok-bot.md' }],
  ])
})
