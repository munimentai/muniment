import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import AgentImport from './AgentImport.svelte'
afterEach(cleanup)

const preview = {
  agent: { id: '', name: 'Dewey', label: 'Mail assistant', instructions: 'Draft replies.', routines: [{ id: 'import-1', name: 'Morning inbox', enabled: false, proposed: true, trigger: { kind: 'cron', expr: '0 8 * * 1-5', tz: 'UTC' } }] },
  memories: [{ title: 'Signature', content: 'First name only.' }],
  skills: { matched: [{ id: 'review', name: 'Review' }], proposals: [{ name: 'Tone', instructions: 'Write plainly.' }] },
  tools: { matched: [], setup: [{ name: 'Gmail' }] },
  routine_notes: [{ name: 'On new mail', note: 'No schedule, webhook or MCP trigger fits this routine. Set one up by hand.' }],
  source: 'grok_link',
  coverage: 'public-profile',
}

it('previews a share link, then imports it without its memories when asked', async () => {
  const requests = []
  const invoke = vi.fn(async (_command, args) => { requests.push(args); return args.operation === 'agent.import' ? preview : { agent: { id: 'new-agent', name: 'Dewey' } } })
  const onimported = vi.fn()
  const onclose = vi.fn()
  render(AgentImport, { tauri: { invoke }, runtimes: [{ id: 'local', name: 'This computer' }, { id: 'box', name: 'Build box' }], onimported, onclose })
  const dialog = screen.getByRole('dialog', { name: 'Import agent' })
  await fireEvent.click(within(dialog).getByRole('button', { name: 'Preview' }))
  expect(await within(dialog).findByRole('alert')).toHaveTextContent('Paste a share link.')
  await fireEvent.input(within(dialog).getByRole('textbox', { name: 'Share link' }), { target: { value: 'https://x.ai/bot/abc' } })
  await fireEvent.change(within(dialog).getByRole('combobox', { name: 'Machine' }), { target: { value: 'box' } })
  await fireEvent.click(within(dialog).getByRole('button', { name: 'Preview' }))
  await within(dialog).findByText('Dewey')
  expect(requests[0]).toEqual({ runtime: 'box', operation: 'agent.import', body: { source: 'https://x.ai/bot/abc' } })
  expect(within(dialog).getByRole('note')).toHaveTextContent('public profile only')
  expect(within(dialog).getByRole('region', { name: 'Routines' })).toHaveTextContent('1 routine arrives off, as proposals to accept.')
  expect(within(dialog).getByRole('region', { name: 'Routines' })).toHaveTextContent('On new mail: No schedule, webhook or MCP trigger fits this routine.')
  expect(within(dialog).getByRole('region', { name: 'Skills' })).toHaveTextContent('Tonekept as reference text')
  expect(within(dialog).getByRole('region', { name: 'Setup' })).toHaveTextContent('Gmailset up in Extend')
  await fireEvent.click(within(dialog).getByRole('checkbox', { name: 'Import 1 memory as outside facts' }))
  await fireEvent.click(within(dialog).getByRole('button', { name: 'Import agent' }))
  await waitFor(() => expect(onimported).toHaveBeenCalledWith({ id: 'new-agent', name: 'Dewey', runtimeId: 'box' }))
  expect(requests[1]).toEqual({ runtime: 'box', operation: 'agent.import_commit', body: { agent: preview.agent } })
  expect(onclose).toHaveBeenCalled()
})

it('reads pasted text and keeps the memories by default', async () => {
  const requests = []
  const invoke = vi.fn(async (_command, args) => { requests.push(args); return args.operation === 'agent.import' ? { ...preview, coverage: 'provided', source: 'markdown' } : { agent: { id: 'x' } } })
  render(AgentImport, { tauri: { invoke }, onclose: vi.fn() })
  await fireEvent.click(screen.getByRole('radio', { name: 'Paste' }))
  await fireEvent.input(screen.getByRole('textbox', { name: 'Template' }), { target: { value: '# Dewey\nDrafts replies.' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
  await screen.findByText('Markdown')
  expect(screen.queryByRole('note')).not.toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button', { name: 'Import agent' }))
  await waitFor(() => expect(requests[1]).toEqual({ runtime: 'local', operation: 'agent.import_commit', body: { agent: preview.agent, memories: preview.memories } }))
})
