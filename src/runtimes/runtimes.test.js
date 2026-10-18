import { describe, expect, it } from 'vitest'
import {
  activeRuntime,
  errorLine,
  kindIcon,
  machineName,
  kindLabel,
  looksLikePairingLink,
  pairedRuntimes,
  remoteSelected,
  statusHint,
  statusLabel,
  statusTone,
  updatableRuntimes,
  versionLabel,
  updateLabel,
  blockersLine,
  canRequestUpdate,
  olderVersion,
  skipLabel,
  settingsMachine,
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

  it('draws each machine as the shape it announces', () => {
    expect(kindIcon({ kind: 'local', formFactor: 'laptop' })).toBe('laptop')
    expect(kindIcon({ kind: 'desktop', formFactor: 'all_in_one' })).toBe('monitor')
    expect(kindIcon({ kind: 'desktop', formFactor: 'desktop' })).toBe('pc-case')
    expect(kindIcon({ kind: 'server' })).toBe('terminal')
    expect(kindIcon({ kind: 'server', formFactor: 'laptop' })).toBe('terminal')
    expect(kindIcon({ kind: 'server', formFactor: 'server' })).toBe('server')
    expect(kindIcon({ kind: 'container', formFactor: 'laptop' })).toBe('container')
    expect(kindIcon({ kind: 'desktop-client' })).toBe('laptop')
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

describe('update labels', () => {
  const machine = (fields) => ({ ...buildBox, ...fields })
  const status = (state, fields = {}) => machine({ updateStatus: { state, targetVersion: '0.2.0', blockers: [], ...fields } })

  it('names each state the machine reports', () => {
    expect(updateLabel(status('queued'))).toEqual({ text: 'Update queued', tooltip: '' })
    expect(updateLabel(status('downloading'))).toEqual({ text: 'Downloading v0.2.0', tooltip: '' })
    expect(updateLabel(status('draining', { blockers: ['active_run'] }))).toEqual({ text: 'Finishing current work', tooltip: 'Waiting for a reply in progress.' })
    expect(updateLabel(status('installing'))).toEqual({ text: 'Updating to v0.2.0', tooltip: '' })
    expect(updateLabel(status('restarting'))).toEqual({ text: 'Updating to v0.2.0', tooltip: '' })
    expect(updateLabel(status('failed', { error: 'Version 0.2.0 did not report healthy.' }))).toEqual({ text: 'Update failed', tooltip: 'Version 0.2.0 did not report healthy.' })
    expect(updateLabel(status('idle', { targetVersion: null }))).toBeNull()
  })

  it('names a request the machine has not acted on and a manual machine that is behind', () => {
    expect(updateLabel(machine({ updateRequested: true }))).toEqual({ text: 'Update requested', tooltip: '' })
    expect(updateLabel(machine({ behind: true, updateCapability: 'manual', updateTarget: '0.2.0' }))).toEqual({ text: 'v0.2.0 available, update manually', tooltip: '' })
    // A machine that is behind and installs releases itself shows only its version.
    expect(updateLabel(machine({ behind: true, updateCapability: 'launcher', updateTarget: '0.2.0' }))).toBeNull()
    // An older runtime without update fields never claims an update.
    expect(updateLabel(buildBox)).toBeNull()
  })

  it('drops a status for a version the machine already runs', () => {
    expect(updateLabel(status('restarting', {}))).not.toBeNull()
    expect(updateLabel({ ...status('restarting'), version: '0.2.0' })).toBeNull()
    expect(updateLabel({ ...status('failed'), version: 'v0.2.0' })).toBeNull()
  })

  it('words the blockers and compares versions', () => {
    expect(blockersLine([])).toBe('')
    expect(blockersLine(['active_run', 'session_refresh', 'in_flight_external_effect'])).toBe('Waiting for a reply in progress, a session refresh and an action in progress.')
    expect(olderVersion('0.1.9', '0.2.0')).toBe(true)
    expect(olderVersion('v0.2.0', '0.2.0')).toBe(false)
    expect(olderVersion('0.10.0', '0.9.0')).toBe(false)
    expect(olderVersion('1.0.0-rc.1', '1.0.0')).toBe(true)
    expect(olderVersion('nightly', '1.0.0')).toBeNull()
  })

  it('offers Update only for a reachable machine that takes requests and has none on the way', () => {
    expect(canRequestUpdate(buildBox)).toBe(true)
    expect(canRequestUpdate(studio)).toBe(false)
    expect(canRequestUpdate(local)).toBe(false)
    expect(canRequestUpdate({ ...buildBox, status: 'offline' })).toBe(false)
    expect(canRequestUpdate(status('downloading'))).toBe(false)
    expect(canRequestUpdate(status('failed'))).toBe(true)
    expect(skipLabel({ name: 'Rack', reason: 'manual' })).toBe('Rack (manual install)')
    expect(skipLabel({ name: 'Lab', reason: 'no_permission' })).toBe('Lab (no permission)')
  })
})

describe('the machine a Settings page applies to', () => {
  it('names the selected machine on machine pages once a machine is paired, with the machines that answer', () => {
    const list = runtimeList([{ ...local, active: false }, studio, { ...buildBox, active: true }], buildBox.id)
    const scope = settingsMachine(list, 'providers')
    expect(scope.selected.name).toBe('Build box')
    expect(scope.choices.map((runtime) => runtime.name)).toEqual(['This computer', 'Build box'])
    expect(settingsMachine(list, 'appearance')).toBeNull()
    expect(settingsMachine(list, 'mcp')).toBeNull()
  })

  it('shows no machine while this computer is the only one', () => {
    expect(settingsMachine(runtimeList([local]), 'providers')).toBeNull()
  })
})

describe('machineName', () => {
  it('names this computer one way, and a runtime with the generic name by its kind', () => {
    expect(machineName({ id: 'local', local: true, name: 'This computer' })).toBe('This computer')
    expect(machineName({ id: 'pod', kind: 'container', name: 'Muniment runtime' })).toBe('Container')
    expect(machineName({ id: 'box', kind: 'server', name: 'Build box' })).toBe('Build box')
    expect(machineName(null)).toBe('Machine')
  })
})
