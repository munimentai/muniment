import { describe, expect, it } from 'vitest'
import {
  activeRuntime,
  errorLine,
  kindIcon,
  kindLabel,
  looksLikePairingLink,
  pairedRuntimes,
  remoteSelected,
  statusHint,
  statusLabel,
  statusTone,
  updatableRuntimes,
  versionLabel,
} from './runtimes.js'

const local = { id: 'local', name: 'This computer', kind: 'local', version: '0.2.0', status: 'online', active: true, local: true, scopes: [], admin: true, updateOffered: false, sshHost: null }
const studio = { id: '018f0000-0000-7000-8000-000000000042', name: 'Studio', kind: 'desktop', version: '0.1.4', status: 'offline', active: false, local: false, scopes: ['run.write', 'thread.read'], admin: false, updateOffered: false, sshHost: null }
const buildBox = { id: '018f0000-0000-7000-8000-000000000043', name: 'Build box', kind: 'container', version: '0.1.0', status: 'online', active: false, local: false, scopes: ['run.write', 'runtime.admin', 'thread.read'], admin: true, updateOffered: true, sshHost: 'build-box' }

function runtimeList(runtimes = [local, studio, buildBox], selected = 'local') {
  return {
    appVersion: '0.2.0',
    deviceName: 'Laptop',
    selected,
    runtimes: runtimes.map((runtime) => ({ ...runtime, active: runtime.id === selected })),
  }
}

describe('runtime labels', () => {
  it('names each kind with its icon and falls back for an unknown kind', () => {
    expect([local, studio, buildBox].map(kindLabel)).toEqual(['This computer', 'Desktop', 'Container'])
    expect([local, studio, buildBox].map(kindIcon)).toEqual(['monitor', 'laptop', 'container'])
    expect(kindLabel({ kind: 'server' })).toBe('Server')
    expect(kindLabel({ kind: null })).toBe('Runtime')
    expect(kindIcon({})).toBe('server')
  })

  it('names each state with its next step and tone', () => {
    expect(statusLabel({ status: 'online' })).toBe('Online')
    expect(statusHint({ status: 'online' })).toBe('')
    expect(statusLabel({ status: 'certificate_changed' })).toBe('Certificate changed')
    expect(statusHint({ status: 'certificate_changed' })).toBe('Pair it again to trust its new certificate.')
    expect(statusTone({ status: 'revoked' })).toBe('critical')
    expect(statusTone({ status: 'offline' })).toBe('caution')
    expect(statusLabel({ status: 'version_mismatch' })).toBe('Version mismatch')
    expect(statusLabel({ status: 'something new' })).toBe('Not checked')
    expect(versionLabel(buildBox)).toBe('v0.1.0')
    expect(versionLabel({ version: 'v2.0.0' })).toBe('v2.0.0')
    expect(versionLabel({ version: null })).toBe('')
  })

  it('finds the selected, paired and updatable runtimes', () => {
    const list = runtimeList()
    expect(activeRuntime(list).id).toBe('local')
    expect(remoteSelected(list)).toBe(false)
    expect(pairedRuntimes(list).map((runtime) => runtime.name)).toEqual(['Studio', 'Build box'])
    expect(updatableRuntimes(list).map((runtime) => runtime.name)).toEqual(['Build box'])
    const remote = runtimeList(undefined, buildBox.id)
    expect(activeRuntime(remote).name).toBe('Build box')
    expect(remoteSelected(remote)).toBe(true)
    expect(remoteSelected(null)).toBe(false)
  })

  it('reads one sentence from a failure and recognizes a pairing link', () => {
    expect(errorLine('The runtime refused the link. It may be used or expired. Create a new link.')).toBe('The runtime refused the link. It may be used or expired. Create a new link.')
    expect(errorLine(new Error('No address answered'))).toBe('No address answered.')
    expect(errorLine('')).toBe('The runtime did not answer.')
    expect(looksLikePairingLink(' muniment://pair?v=1&id=x#secret ')).toBe(true)
    expect(looksLikePairingLink('https://example.com')).toBe(false)
  })
})
