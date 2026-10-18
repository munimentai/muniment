import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import AddMachine from './AddMachine.svelte'

afterEach(cleanup)

const LINK = `muniment://pair?v=1&id=018f0000-0000-7000-8000-000000000044&name=Laptop&fp=${'00'.repeat(32)}&host=192.0.2.20%3A7443#${'ab'.repeat(16)}`

function access(enabled, overrides = {}) {
  return {
    settings: { enabled, listen: null, endpoints: [] },
    overridden: false,
    runtime_id: '018f0000-0000-7000-8000-000000000044',
    runtime_name: 'Laptop',
    certificate_sha256: '00'.repeat(32),
    listening: enabled ? '[::]:7443' : null,
    endpoints: enabled ? [{ endpoint: '192.0.2.20:7443', kind: 'lan' }, { endpoint: '100.64.0.7:7443', kind: 'overlay' }] : [],
    error: null,
    ...overrides,
  }
}

function bridge({ enabled = false, devices = [] } = {}) {
  let current = access(enabled)
  let listed = devices
  const invoke = vi.fn(async (command, payload) => {
    if (command === 'local_access_settings') return current
    if (command === 'local_access_save') {
      current = access(payload.enabled, { settings: { enabled: payload.enabled, listen: payload.port && payload.port !== 7443 ? `[::]:${payload.port}` : null, endpoints: payload.endpoints } })
      return current
    }
    if (command === 'local_devices') return { devices: listed }
    if (command === 'local_device_revoke') { listed = listed.filter((device) => device.device_id !== payload.deviceId); return true }
    if (command === 'local_pairing_link') return { link: LINK, qrSvg: '<svg role="img" aria-label="Pairing code"></svg>', expiresAt: '2030-01-01T12:05:00Z', scopes: payload.admin ? ['run.write', 'runtime.admin', 'thread.read'] : ['run.write', 'thread.read'] }
    throw new Error(`unexpected command: ${command}`)
  })
  return { invoke, pairDevice: (device) => { listed = [...listed, device] } }
}

describe('Add machine', () => {
  it('shows a code after one click while connections are off, turning them on the first time', async () => {
    const tauri = bridge()
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    render(AddMachine, { tauri })
    const show = await screen.findByRole('button', { name: 'Show code' })
    await waitFor(() => expect(show).toBeEnabled())
    expect(tauri.invoke).not.toHaveBeenCalledWith('local_pairing_link', expect.anything())
    await fireEvent.click(show)
    // The code carries admin access, so the other machine joins as it pairs.
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith('local_pairing_link', { admin: true }))
    expect(tauri.invoke).toHaveBeenCalledWith('local_access_save', { enabled: true, port: null, endpoints: [] })
    expect(await screen.findByRole('img', { name: 'Pairing code' })).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Link from this computer' })).toHaveValue(LINK)
    expect(screen.getByRole('timer')).toHaveTextContent(/^Works once · expires in \d+:\d\d$/)
    await fireEvent.click(screen.getByRole('button', { name: 'Copy' }))
    expect(writeText).toHaveBeenCalledWith(LINK)
    expect(await screen.findByRole('button', { name: 'Copied' })).toBeInTheDocument()
    expect(localStorage.length).toBe(0)
  })

  it('shows a code at once while connections are on, and a new one on request', async () => {
    const tauri = bridge({ enabled: true })
    render(AddMachine, { tauri })
    expect(await screen.findByRole('img', { name: 'Pairing code' })).toBeInTheDocument()
    await fireEvent.click(screen.getByRole('button', { name: 'New code' }))
    await waitFor(() => expect(tauri.invoke.mock.calls.filter(([command]) => command === 'local_pairing_link')).toHaveLength(2))
    expect(tauri.invoke).not.toHaveBeenCalledWith('local_access_save', expect.anything())
  })

  it('closes a code once a device uses it and names that device', async () => {
    const tauri = bridge({ enabled: true })
    const onadded = vi.fn()
    render(AddMachine, { tauri, onadded })
    expect(await screen.findByRole('img', { name: 'Pairing code' })).toBeInTheDocument()
    tauri.pairDevice({ device_id: 'd2', name: 'Studio', kind: 'desktop-client', scopes: ['run.write', 'runtime.admin', 'thread.read'], created_at: '2030-01-01T00:00:00Z', last_seen_at: null })
    expect(await screen.findByRole('status', {}, { timeout: 4000 })).toHaveTextContent('Studio added.')
    expect(screen.queryByRole('img', { name: 'Pairing code' })).toBeNull()
    expect(onadded).toHaveBeenCalled()
  })

  it('names a port another program holds and opens the network settings', async () => {
    const tauri = bridge()
    const onnetwork = vi.fn()
    tauri.invoke.mockImplementationOnce(async () => access(false))
    tauri.invoke.mockImplementationOnce(async () => access(false))
    tauri.invoke.mockImplementationOnce(async () => access(true, { listening: null, endpoints: [], error: 'The listener could not bind 0.0.0.0:7443: remote attach listener bind failed: Address already in use (os error 48)' }))
    render(AddMachine, { tauri, onnetwork })
    const show = await screen.findByRole('button', { name: 'Show code' })
    await waitFor(() => expect(show).toBeEnabled())
    await fireEvent.click(show)
    expect(await screen.findByText("Another program uses port 7443. Choose another port in this computer's network settings.")).toBeInTheDocument()
    expect(tauri.invoke).not.toHaveBeenCalledWith('local_pairing_link', expect.anything())
    expect(onnetwork).toHaveBeenCalled()
  })
})
