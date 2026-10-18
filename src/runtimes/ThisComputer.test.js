import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ThisComputer from './ThisComputer.svelte'
import { endpointKindLabel, listenPort, seenLabel } from './runtimes.js'

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
    if (command === 'agent_request') throw new Error('This runtime is not in a fleet.')
    if (command === 'local_pairing_link') return { link: LINK, qrSvg: '<svg role="img" aria-label="Pairing code"></svg>', expiresAt: '2030-01-01T12:05:00Z', scopes: payload.admin ? ['run.write', 'runtime.admin', 'thread.read'] : ['run.write', 'thread.read'] }
    throw new Error(`unexpected command: ${command}`)
  })
  return { invoke, pairDevice: (device) => { listed = [...listed, device] } }
}

describe('This computer', () => {
  it('lists its addresses with their kinds and saves the port and the addresses a code names', async () => {
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

  it('names the reason connections cannot start', async () => {
    const tauri = bridge({ enabled: true })
    tauri.invoke.mockImplementationOnce(async () => access(true, { listening: null, error: 'Address already in use' }))
    render(ThisComputer, { tauri })
    expect(await screen.findByRole('alert')).toHaveTextContent('Connections cannot start. Address already in use.')
  })
})

describe('This computer labels', () => {
  it('names endpoint kinds, ports and last seen times', () => {
    expect(endpointKindLabel('overlay')).toBe('Private overlay network')
    expect(endpointKindLabel('loopback')).toBe('This computer only')
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
