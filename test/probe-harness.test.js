// @vitest-environment jsdom

import fs from 'node:fs'
import path from 'node:path'

import { cleanup, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'

import { buildProbeCommandTable } from './probe/stub.js'

const source = fs.readFileSync(path.join(process.cwd(), 'test/probe/stub.js'), 'utf8')
const readyMarker = "document.body.dataset.probeReady = ''"
import App from '../src/App.svelte'

vi.mock('@tauri-apps/plugin-global-shortcut', () => ({
  register: vi.fn().mockResolvedValue(undefined),
  unregister: vi.fn().mockResolvedValue(undefined),
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn().mockResolvedValue(null) }))
vi.mock('@tauri-apps/api/event', () => ({listen: vi.fn().mockResolvedValue(vi.fn())}))
vi.mock('@tauri-apps/api/window', () => ({
  UserAttentionType: { Informational: 2 },
  getCurrentWindow: () => ({
    onMoved: vi.fn().mockResolvedValue(vi.fn()),
    onResized: vi.fn().mockResolvedValue(vi.fn()),
    isFocused: vi.fn().mockResolvedValue(true),
    requestUserAttention: vi.fn().mockResolvedValue(undefined),
  }),
}))
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({ onDragDropEvent: vi.fn().mockResolvedValue(vi.fn()) }),
}))

function installTable(fixtureName) {
  const table = buildProbeCommandTable(fixtureName)
  window.__TAURI__ = {
    core: { invoke: table.invoke },
    event: {
      listen: vi.fn(async (event, listener) => {
        const entry = { event, listener }
        table.eventListeners.push(entry)
        return () => {
          const index = table.eventListeners.indexOf(entry)
          if (index !== -1) table.eventListeners.splice(index, 1)
        }
      }),
    },
  }
  window.__TAURI_INTERNALS__ = {
    invoke: vi.fn().mockResolvedValue(null),
    transformCallback: vi.fn(),
  }
  return table
}

beforeAll(async () => {
  HTMLElement.prototype.scrollTo = vi.fn()
})

afterEach(() => {
  cleanup()
  localStorage.clear()
})

describe('probe harness', () => {
  it('sets the ready marker only after the font set settles', () => {
    expect(source).toMatch(/async function markProbeReady\(\) \{\s*await document\.fonts\.ready\s*document\.body\.dataset\.probeReady = ''\s*\}/)
    expect(source.split(readyMarker)).toHaveLength(2)
  })

  it('drives every fixture page from the shared stub and its data value', () => {
    const pages = fs.readdirSync(path.join(process.cwd(), 'test/probe'))
      .filter((name) => name.endsWith('.html'))
    for (const name of pages) {
      const page = fs.readFileSync(path.join(process.cwd(), 'test/probe', name), 'utf8')
      expect(page).toMatch(/<script type="module" src="\.\/stub\.js" data-history="[^"]+"><\/script>/)
      expect(page).not.toContain('probeReady')
    }
  })

  it('reports a started attachment listener', async () => {
    const table = buildProbeCommandTable('restored')
    await expect(table.invoke('attach_listener_status')).resolves.toEqual({
      started: true,
      failure: null,
      connected: false,
      supervisor_running: false,
    })
  })

  it('answers every local mode command', async () => {
    const table = buildProbeCommandTable('local-mode')
    await expect(table.invoke('local_mode_status')).resolves.toBe(true)
    await expect(table.invoke('local_mode_enter')).resolves.toBeNull()
    await expect(table.invoke('local_mode_leave')).resolves.toBeNull()
    await expect(table.invoke('local_mode_store_provider_key', { provider: 'google', key: 'test' })).resolves.toBeNull()
  })

  it('registers the launcher shortcut without an unknown command', async () => {
    const table = buildProbeCommandTable('restored')
    await expect(table.invoke('launcher_register')).resolves.toBeNull()
    expect(table.unknownCommands).toEqual([])
  })

  it('reports no paired phone in the Account fixture', async () => {
    const table = buildProbeCommandTable('access')
    await expect(table.invoke('auth_pairing_status')).resolves.toEqual({ pair: null })
    expect(table.unknownCommands).toEqual([])
  })

  it('rejects an unknown core command with its name', async () => {
    const table = buildProbeCommandTable('restored')
    await expect(table.invoke('missing_probe_command')).rejects.toThrow('Unknown probe command: missing_probe_command')
  })

  it('renders the signed-in workspace from the restored command table', async () => {
    installTable('restored')
    render(App)

    expect(await screen.findByPlaceholderText('Ask anything')).toBeInTheDocument()
    expect(await screen.findByText('Find the renewal terms in the lease.')).toBeInTheDocument()
    expect(screen.queryByText('Sign in')).not.toBeInTheDocument()
  })

  it('renders the local mode workspace from the local command table', async () => {
    installTable('local-mode')
    render(App)

    expect(await screen.findByTestId('local-mode')).toBeInTheDocument()
    expect(screen.getByText('The local notes list the lease renewal date and notice period.')).toBeInTheDocument()
    expect(screen.queryByText('Sign in')).not.toBeInTheDocument()
  })
})
