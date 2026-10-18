import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ThisComputer from './ThisComputer.svelte'
import { endpointKindLabel, listenPort, scopeLabels, seenLabel } from './runtimes.js'

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

describe('This computer', () => {
  it('shows a pairing code in one click, turning on connections the first time', async () => {
    const tauri = bridge()
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    render(ThisComputer, { tauri })
    const accept = await screen.findByRole('switch', { name: 'Accept connections' })
    await waitFor(() => expect(accept).toBeEnabled())
    expect(accept).toHaveAttribute('aria-checked', 'false')
    expect(screen.getByText('No other device can reach this computer.')).toBeInTheDocument()

    await fireEvent.click(screen.getByRole('button', { name: 'Show pairing code' }))
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith('local_pairing_link', { admin: false }))
    expect(tauri.invoke).toHaveBeenCalledWith('local_access_save', { enabled: true, port: null, endpoints: [] })
    expect(await screen.findByRole('img', { name: 'Pairing code' })).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Pairing link' })).toHaveValue(LINK)
    expect(screen.getByRole('timer')).toHaveTextContent(/^Works once · expires in \d+:\d\d$/)
    await fireEvent.click(screen.getByRole('button', { name: 'Copy' }))
    expect(writeText).toHaveBeenCalledWith(LINK)
    expect(await screen.findByRole('button', { name: 'Copied' })).toBeInTheDocument()
    expect(localStorage.length).toBe(0)

    await fireEvent.click(screen.getByRole('button', { name: 'Stop pairing' }))
    expect(screen.queryByRole('img', { name: 'Pairing code' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Show pairing code' })).toBeInTheDocument()
  })

  it('makes a new code on request', async () => {
    const tauri = bridge({ enabled: true })
    render(ThisComputer, { tauri })
    const show = await screen.findByRole('button', { name: 'Show pairing code' })
    await waitFor(() => expect(show).toBeEnabled())
    await fireEvent.click(show)
    await fireEvent.click(await screen.findByRole('button', { name: 'New code' }))
    await waitFor(() => expect(tauri.invoke.mock.calls.filter(([command]) => command === 'local_pairing_link')).toHaveLength(2))
    expect(tauri.invoke).not.toHaveBeenCalledWith('local_access_save', expect.anything())
  })

  it('lists its addresses with their kinds and moves the port under Network', async () => {
    const tauri = bridge({ enabled: true })
    render(ThisComputer, { tauri })
    expect(await screen.findByText(/Private overlay network/)).toBeInTheDocument()
    expect(screen.getByText(/Local network/)).toBeInTheDocument()
    const port = screen.getByRole('textbox', { name: 'Port' })
    await waitFor(() => expect(port).toHaveValue('7443'))
    await fireEvent.input(port, { target: { value: '9443' } })
    await fireEvent.input(screen.getByRole('textbox', { name: 'Addresses in the link' }), { target: { value: 'build.example.net:9443, 192.0.2.30:9443' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    expect(tauri.invoke).toHaveBeenCalledWith('local_access_save', { enabled: true, port: 9443, endpoints: ['build.example.net:9443', '192.0.2.30:9443'] })
  })

  it('lists devices using this computer and revokes one after asking once', async () => {
    const tauri = bridge({ devices: [
      { device_id: 'd1', name: 'Studio', kind: 'desktop-client', scopes: ['run.write', 'thread.read'], created_at: '2030-01-01T00:00:00Z', last_seen_at: null },
    ] })
    render(ThisComputer, { tauri })
    const row = await screen.findByRole('group', { name: 'Studio' })
    expect(within(row).getByText('Never seen')).toBeInTheDocument()
    await fireEvent.click(within(row).getByRole('button', { name: 'Revoke Studio' }))
    expect(tauri.invoke).not.toHaveBeenCalledWith('local_device_revoke', expect.anything())
    await fireEvent.click(screen.getByRole('button', { name: 'Revoke' }))
    expect(tauri.invoke).toHaveBeenCalledWith('local_device_revoke', { deviceId: 'd1' })
    await waitFor(() => expect(screen.queryByText('Devices using this computer')).toBeNull())
  })

  it('names the reason connections cannot start', async () => {
    const tauri = bridge({ enabled: true })
    tauri.invoke.mockImplementationOnce(async () => access(true, { listening: null, error: 'Address already in use' }))
    render(ThisComputer, { tauri })
    expect(await screen.findByRole('alert')).toHaveTextContent('Connections cannot start. Address already in use.')
  })
})

describe('This computer while a code shows', () => {
  it('closes a code once a device uses it and lists that device', async () => {
    const tauri = bridge({ enabled: true })
    render(ThisComputer, { tauri })
    await fireEvent.click(await screen.findByRole('button', { name: 'Show pairing code' }))
    expect(await screen.findByRole('img', { name: 'Pairing code' })).toBeInTheDocument()
    expect(screen.queryByText('Devices using this computer')).toBeNull()
    tauri.pairDevice({ device_id: 'd2', name: 'Studio', kind: 'desktop-client', scopes: ['run.write', 'thread.read'], created_at: '2030-01-01T00:00:00Z', last_seen_at: null })
    expect(await screen.findByRole('status', {}, { timeout: 4000 })).toHaveTextContent('Studio paired with this computer.')
    expect(screen.queryByRole('img', { name: 'Pairing code' })).toBeNull()
    expect(screen.getByRole('group', { name: 'Studio' })).toBeInTheDocument()
  })

  it('names a port another program holds instead of asking for an address', async () => {
    const tauri = bridge({ enabled: true })
    tauri.invoke.mockImplementationOnce(async () => access(true, { listening: null, endpoints: [], error: 'The listener could not bind 0.0.0.0:7443: remote attach listener bind failed: Address already in use (os error 48)' }))
    render(ThisComputer, { tauri })
    const show = await screen.findByRole('button', { name: 'Show pairing code' })
    await waitFor(() => expect(show).toBeEnabled())
    await fireEvent.click(show)
    expect(await screen.findByText('Another program uses port 7443. Choose another port under Network.')).toBeInTheDocument()
    expect(tauri.invoke).not.toHaveBeenCalledWith('local_pairing_link', expect.anything())
    expect(screen.getByText('Network').closest('details')).toHaveAttribute('open')
  })
})

describe('This computer labels', () => {
  it('names endpoint kinds, scopes, ports and last seen times', () => {
    expect(endpointKindLabel('overlay')).toBe('Private overlay network')
    expect(endpointKindLabel('loopback')).toBe('This computer only')
    expect(scopeLabels(['thread.read', 'runtime.admin'])).toBe('Read, Updates and pairing')
    expect(scopeLabels([])).toBe('No access')
    expect(listenPort({ settings: { listen: '127.0.0.1:9000' } })).toBe(9000)
    expect(listenPort({ settings: { listen: null }, listening: '[::]:7443' })).toBe(7443)
    expect(listenPort(null)).toBeNull()
    const now = Date.parse('2030-01-01T12:00:00Z')
    expect(seenLabel('2030-01-01T11:59:30Z', now)).toBe('Now')
    expect(seenLabel('2030-01-01T11:30:00Z', now)).toBe('30m ago')
    expect(seenLabel('2030-01-01T09:00:00Z', now)).toBe('3h ago')
    expect(seenLabel(null, now)).toBe('Never seen')
  })
})
