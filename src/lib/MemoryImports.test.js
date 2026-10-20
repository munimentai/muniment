import '@testing-library/jest-dom/vitest'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte'
import { open } from '@tauri-apps/plugin-dialog'
import MemoryImports from './MemoryImports.svelte'

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }))

const finding = (assistantId, displayName, fileCount, byteTotal = 1024) => ({
  assistantId, displayName, fileCount, byteTotal, root: `/Users/alex/.${assistantId}`, capped: false, warnings: [],
})
const report = { findings: [finding('claude-code', 'Claude Code', 12), finding('obsidian', 'Obsidian', 400, 900_000)], errors: [] }
const manifest = (entries) => ({ entries: entries.map(([name, byteSize]) => ({ name, kind: 'markdown', byteSize, excerpt: 'never shown', excerptTruncated: false })), totalByteSize: 0 })

let answers
let invoke

function deferred() {
  let resolve
  const promise = new Promise((done) => { resolve = done })
  return { promise, resolve }
}

beforeEach(() => {
  answers = { onboarding_scan: () => report }
  invoke = vi.fn((command, payload) => {
    const answer = answers[command]
    if (!answer) return Promise.reject(new Error(`unexpected ${command}`))
    return Promise.resolve().then(() => answer(payload))
  })
  open.mockReset()
})

afterEach(cleanup)

async function scanned() {
  render(MemoryImports, { tauri: { invoke } })
  await fireEvent.click(screen.getByRole('button', { name: 'Look for memory' }))
  return screen.findByRole('group', { name: 'Claude Code' })
}

it('invokes nothing before the scan', () => {
  render(MemoryImports, { tauri: { invoke } })
  expect(invoke).not.toHaveBeenCalled()
  expect(screen.queryByRole('button', { name: 'Import' })).not.toBeInTheDocument()
})

it('imports one assistant once and disables the button while it copies', async () => {
  const pending = deferred()
  answers.memory_import_assistant = () => pending.promise
  const row = await scanned()
  const button = within(row).getByRole('button', { name: 'Import' })
  await fireEvent.click(button)
  await fireEvent.click(button)
  expect(invoke.mock.calls.filter(([command]) => command === 'memory_import_assistant')).toEqual([['memory_import_assistant', { assistantId: 'claude-code' }]])
  expect(button).toBeDisabled()
  expect(within(row).getByRole('status')).toHaveTextContent('Copying files…')
  pending.resolve({ importedFileCount: 12, alreadyCopiedCount: 9, leftOut: [], destination: 'memory/imports/claude-code' })
  expect(await within(row).findByText(/^Copied/)).toHaveTextContent('Copied 12 files to memory/imports/claude-code.')
  expect(within(row).getByText('memory/imports/claude-code')).toHaveClass('mono')
  expect(within(row).getByText(/already copied/)).toHaveTextContent('9 files were already copied.')
  expect(button).toBeEnabled()
})

it('names each file it left out and the reason', async () => {
  answers.memory_import_assistant = () => ({
    importedFileCount: 0,
    alreadyCopiedCount: 0,
    leftOut: [
      { sourceName: 'rules/keys.md', reason: 'secret' },
      { sourceName: 'notes.bin', reason: 'notText' },
      { sourceName: 'big.md', reason: 'tooLarge' },
    ],
    destination: 'memory/imports/claude-code',
  })
  const row = await scanned()
  await fireEvent.click(within(row).getByRole('button', { name: 'Import' }))
  expect(await within(row).findByText('Nothing new to copy.')).toBeInTheDocument()
  const lines = within(row).getAllByRole('listitem').map((item) => item.textContent)
  expect(lines).toEqual(['rules/keys.md holds a secret.', 'notes.bin is not text.', 'big.md is over 64 KB.'])
})

it('offers no import for a store over the copy caps', async () => {
  await scanned()
  const row = screen.getByRole('group', { name: 'Obsidian' })
  expect(within(row).getByText('Too large to copy. Copy takes up to 128 files.')).toBeInTheDocument()
  expect(within(row).queryByRole('button')).not.toBeInTheDocument()
})

it('shows an error and offers to import again', async () => {
  answers.memory_import_assistant = () => Promise.reject({ kind: 'saveFailed', message: 'The memory files could not be copied.' })
  const row = await scanned()
  await fireEvent.click(within(row).getByRole('button', { name: 'Import' }))
  expect(await within(row).findByRole('alert')).toHaveTextContent('Muniment could not copy the Claude Code files. Try again.')
  answers.memory_import_assistant = () => ({ importedFileCount: 1, alreadyCopiedCount: 0, leftOut: [], destination: 'memory/imports/claude-code' })
  await fireEvent.click(within(row).getByRole('button', { name: 'Import again' }))
  expect(await within(row).findByText(/^Copied/)).toHaveTextContent('Copied 1 file to memory/imports/claude-code.')
})

it('clears the results when it looks again', async () => {
  answers.memory_import_assistant = () => ({ importedFileCount: 1, alreadyCopiedCount: 0, leftOut: [], destination: 'memory/imports/claude-code' })
  const row = await scanned()
  await fireEvent.click(within(row).getByRole('button', { name: 'Import' }))
  await within(row).findByText(/^Copied/)
  await fireEvent.click(screen.getByRole('button', { name: 'Look again' }))
  await screen.findByRole('group', { name: 'Claude Code' })
  expect(screen.queryByText(/^Copied/)).not.toBeInTheDocument()
})

it('copies a picked export ZIP through the preview, the extract and the confirmed import', async () => {
  open.mockResolvedValue('/Users/alex/Downloads/claude-export.zip')
  answers.onboarding_import_preview = () => manifest([['memory.md', 2048], ['huge.md', 65_000]])
  answers.onboarding_import_extract = ({ selectedNames }) => selectedNames.map((name) => ({ sourceName: name, kind: 'markdown', text: 'body', sourceProvenance: `zip:${name}` }))
  answers.home_status = () => ({ configured: true, homePath: '/Users/alex/Documents/muniment' })
  answers.home_confirm_import = ({ approvedEntries }) => ({ configured: true, importedFileCount: approvedEntries.length, alreadyCopiedCount: 0 })
  render(MemoryImports, { tauri: { invoke } })
  await fireEvent.click(screen.getByRole('button', { name: 'Import a ZIP…' }))
  expect(open).toHaveBeenCalledWith({ multiple: false, directory: false, filters: [{ name: 'ZIP', extensions: ['zip'] }] })
  const row = await screen.findByRole('group', { name: 'claude-export.zip' })
  expect(await within(row).findByText('2 files')).toBeInTheDocument()
  expect(within(row).getByText('~/Downloads/claude-export.zip')).toBeInTheDocument()
  expect(row).not.toHaveTextContent('never shown')
  await fireEvent.click(within(row).getByRole('button', { name: 'Import' }))
  expect(await within(row).findByText(/^Copied/)).toHaveTextContent('Copied 1 file to memory/imports/export.')
  expect(within(row).getByRole('listitem')).toHaveTextContent('huge.md is over 64 KB.')
  expect(invoke).toHaveBeenCalledWith('onboarding_import_preview', { archivePath: '/Users/alex/Downloads/claude-export.zip' })
  expect(invoke).toHaveBeenCalledWith('onboarding_import_extract', { archivePath: '/Users/alex/Downloads/claude-export.zip', selectedNames: ['memory.md'] })
  expect(invoke).toHaveBeenCalledWith('home_confirm_import', {
    homePath: '/Users/alex/Documents/muniment',
    approvedEntries: [{ sourceName: 'memory.md', kind: 'markdown', text: 'body', sourceProvenance: 'zip:memory.md' }],
  })
  expect(row).not.toHaveTextContent('body')
})

it('leaves out a ZIP file the confirmed import reads as a secret and copies the rest', async () => {
  open.mockResolvedValue('/Users/alex/claude-export.zip')
  answers.onboarding_import_preview = () => manifest([['keys.md', 100], ['memory.md', 100]])
  answers.onboarding_import_extract = ({ selectedNames }) => selectedNames.map((name) => ({ sourceName: name, kind: 'markdown', text: 'body', sourceProvenance: `zip:${name}` }))
  answers.home_status = () => ({ configured: true, homePath: '/Home' })
  answers.home_confirm_import = ({ approvedEntries }) => approvedEntries.some((entry) => entry.sourceName === 'keys.md')
    ? Promise.reject({ kind: 'secretRejected', sourceName: 'keys.md' })
    : { configured: true, importedFileCount: approvedEntries.length, alreadyCopiedCount: 0 }
  render(MemoryImports, { tauri: { invoke } })
  await fireEvent.click(screen.getByRole('button', { name: 'Import a ZIP…' }))
  const row = await screen.findByRole('group', { name: 'claude-export.zip' })
  await fireEvent.click(await within(row).findByRole('button', { name: 'Import' }))
  expect(await within(row).findByText(/^Copied/)).toHaveTextContent('Copied 1 file to memory/imports/export.')
  expect(within(row).getByRole('listitem')).toHaveTextContent('keys.md holds a secret.')
})

it('does nothing when the ZIP picker is cancelled', async () => {
  open.mockResolvedValue(null)
  render(MemoryImports, { tauri: { invoke } })
  await fireEvent.click(screen.getByRole('button', { name: 'Import a ZIP…' }))
  await Promise.resolve()
  expect(invoke).not.toHaveBeenCalled()
  expect(screen.queryByRole('group')).not.toBeInTheDocument()
})

it('offers no import for a ZIP over the copy caps', async () => {
  open.mockResolvedValue('/Users/alex/big.zip')
  answers.onboarding_import_preview = () => manifest(Array.from({ length: 129 }, (_, index) => [`note-${index}.md`, 10]))
  render(MemoryImports, { tauri: { invoke } })
  await fireEvent.click(screen.getByRole('button', { name: 'Import a ZIP…' }))
  const row = await screen.findByRole('group', { name: 'big.zip' })
  expect(await within(row).findByText('Too large to copy. Copy takes up to 128 files.')).toBeInTheDocument()
  expect(within(row).queryByRole('button')).not.toBeInTheDocument()
})

it('names the ZIP that holds an unsupported entry', async () => {
  open.mockResolvedValue('/Users/alex/photos.zip')
  answers.onboarding_import_preview = () => Promise.reject({ kind: 'unsupportedEntry', message: 'The archive contains an unsupported entry.' })
  render(MemoryImports, { tauri: { invoke } })
  await fireEvent.click(screen.getByRole('button', { name: 'Import a ZIP…' }))
  const row = await screen.findByRole('group', { name: 'photos.zip' })
  expect(await within(row).findByRole('alert')).toHaveTextContent('photos.zip holds a file that is not text, Markdown or JSON.')
  expect(within(row).queryByRole('button')).not.toBeInTheDocument()
})
