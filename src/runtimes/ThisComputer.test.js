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
  return { invoke }
}

describe('This computer', () => {
  it('keeps pairing off until the user turns it on, then lists its addresses with their kinds', async () => {
    const tauri = bridge()
    render(ThisComputer, { tauri })
    const toggle = await screen.findByRole('switch', { name: 'Allow pairing' })
    await waitFor(() => expect(toggle).toBeEnabled())
    expect(toggle).toHaveAttribute('aria-checked', 'false')
    expect(screen.getByText('No other computer can reach this runtime.')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Create pairing link' })).toBeNull()

    await fireEvent.click(toggle)
    expect(tauri.invoke).toHaveBeenCalledWith('local_access_save', { enabled: true, port: null, endpoints: [] })
    expect(await screen.findByText('Private overlay network')).toBeInTheDocument()
    expect(within(screen.getByRole('group', { name: '192.0.2.20:7443' })).getByText('Local network')).toBeInTheDocument()
    expect(screen.getByText('[::]:7443')).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Listen port' })).toHaveValue('7443')
  })

  it('moves the listen port and names its own addresses', async () => {
    const tauri = bridge({ enabled: true })
    render(ThisComputer, { tauri })
    const port = await screen.findByRole('textbox', { name: 'Listen port' })
    await waitFor(() => expect(port).toHaveValue('7443'))
    await fireEvent.input(port, { target: { value: '9443' } })
    await fireEvent.input(screen.getByRole('textbox', { name: 'Addresses links name' }), { target: { value: 'build.example.net:9443, 192.0.2.30:9443' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    expect(tauri.invoke).toHaveBeenCalledWith('local_access_save', { enabled: true, port: 9443, endpoints: ['build.example.net:9443', '192.0.2.30:9443'] })
  })

  it('creates a one-time link with its QR code, expiry and copy', async () => {
    const tauri = bridge({ enabled: true })
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    render(ThisComputer, { tauri })
    await fireEvent.click(await screen.findByRole('switch', { name: 'Allow pairing management' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Create pairing link' }))
    expect(tauri.invoke).toHaveBeenCalledWith('local_pairing_link', { admin: true })
    expect(await screen.findByRole('img', { name: 'Pairing code' })).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Link to open on the other computer' })).toHaveValue(LINK)
    expect(screen.getByText(/^Works once, until/)).toBeInTheDocument()
    await fireEvent.click(screen.getByRole('button', { name: 'Copy link' }))
    expect(writeText).toHaveBeenCalledWith(LINK)
    expect(await screen.findByRole('button', { name: 'Copied' })).toBeInTheDocument()
    expect(localStorage.length).toBe(0)
  })

  it('lists paired devices and revokes one after asking once', async () => {
    const tauri = bridge({ devices: [
      { device_id: 'd1', name: 'Studio', kind: 'desktop-client', scopes: ['run.write', 'thread.read'], created_at: '2030-01-01T00:00:00Z', last_seen_at: null },
    ] })
    render(ThisComputer, { tauri })
    const row = await screen.findByRole('group', { name: 'Studio' })
    expect(within(row).getByText('desktop-client')).toBeInTheDocument()
    expect(within(row).getByText('Read, Run · Not seen yet')).toBeInTheDocument()
    await fireEvent.click(within(row).getByRole('button', { name: 'Revoke Studio' }))
    expect(tauri.invoke).not.toHaveBeenCalledWith('local_device_revoke', expect.anything())
    await fireEvent.click(screen.getByRole('button', { name: 'Revoke' }))
    expect(tauri.invoke).toHaveBeenCalledWith('local_device_revoke', { deviceId: 'd1' })
    expect(await screen.findByText('No device is paired with this computer.')).toBeInTheDocument()
  })

  it('names the reason pairing cannot listen', async () => {
    const tauri = bridge({ enabled: true })
    tauri.invoke.mockImplementationOnce(async () => access(true, { listening: null, error: 'Address already in use' }))
    render(ThisComputer, { tauri })
    expect(await screen.findByRole('alert')).toHaveTextContent('Pairing cannot listen. Address already in use.')
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
    expect(seenLabel('2030-01-01T11:59:30Z', now)).toBe('Seen just now')
    expect(seenLabel('2030-01-01T11:30:00Z', now)).toBe('Seen 30 minutes ago')
    expect(seenLabel('2030-01-01T09:00:00Z', now)).toBe('Seen 3 hours ago')
    expect(seenLabel(null, now)).toBe('Not seen yet')
  })
})
