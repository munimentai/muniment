// The one list on Settings → Machines: this computer first, then every
// machine this app reaches, every other member of this computer's fleet, and
// every device that uses this computer. Core owns pairing, links and access.
// This module merges their answers into one row per machine.
import { directLinks, memberMachine } from '../agents/swarm.js'
import { kindIcon, kindLabel, machineName, versionLabel } from './runtimes.js'
import { platformDisplayName } from '../lib/auth-state.js'

// The device kind core gives the link between two fleet members. A member
// shows as its own row, so its link does not.
const PEER_KIND = 'swarm-peer'
const ADMIN = 'runtime.admin'

const DEVICE_KINDS = {
  'desktop-client': ['desktop', 'laptop'],
  desktop: ['desktop', 'laptop'],
  macos: ['desktop', 'laptop'],
  windows: ['desktop', 'laptop'],
  linux: ['desktop', 'laptop'],
  mobile: ['phone', 'smartphone'],
  phone: ['phone', 'smartphone'],
  ios: ['phone', 'smartphone'],
  android: ['phone', 'smartphone'],
}

// The kind chip of a row: a word for what the machine is.
const KIND_WORDS = { desktop: 'Desktop', server: 'Server', container: 'Container', phone: 'Phone', device: 'Device' }
export function kindWord(kind) {
  return KIND_WORDS[kind] ?? ''
}

function deviceKind(kind) {
  return DEVICE_KINDS[kind] ?? ['device', 'monitor']
}

const sameName = (a, b) => String(a ?? '').trim().toLowerCase() === String(b ?? '').trim().toLowerCase()

// One row per machine, each { key, name, kind, icon, version, local, runtime,
// coreId, deviceId, pairId, online, limited, seenAt }. `runtime` is the `runtime_list`
// entry of a machine this app reaches, `coreId` its fleet member id, and
// `deviceId` the access it holds on this computer. A device that uses this
// computer and a fleet member or reached machine with the same name are one
// machine, so they share a row.
// With MuniCloud on, the devices on the account that are not this computer
// and the phone paired through the relay are rows too, so every machine and
// device is in this one list. `pairId` names the phone's pairing.
export function machineRows({ list, swarm = null, devices = [], cloudDevices = [], phone = null }) {
  const rows = []
  const shown = new Set()
  for (const runtime of list?.runtimes ?? []) {
    const coreId = swarm?.self?.[runtime.id] ?? null
    if (coreId) shown.add(coreId)
    const member = coreId ? swarm.members?.find((entry) => entry.runtime_id === coreId) : null
    rows.push({
      key: runtime.id,
      name: machineName(runtime),
      kind: runtime.local ? 'local' : runtime.kind === 'container' || runtime.kind === 'server' ? runtime.kind : 'desktop',
      icon: kindIcon(runtime),
      version: versionLabel(runtime),
      local: !!runtime.local,
      runtime,
      coreId,
      deviceId: null,
      online: runtime.local || runtime.status === 'online',
      limited: !runtime.local && !runtime.admin,
      seenAt: member?.heartbeat_at ?? null,
    })
  }
  for (const member of swarm?.members ?? []) {
    if (shown.has(member.runtime_id) || member.fleet?.removed) continue
    shown.add(member.runtime_id)
    const machine = memberMachine(swarm, member.runtime_id, list)
    const kind = machine.kind === 'container' ? 'container' : machine.formFactor === 'server' ? 'server' : 'desktop'
    rows.push({
      key: member.runtime_id,
      name: machine.name,
      kind,
      icon: kindIcon(machine),
      version: '',
      local: false,
      runtime: null,
      coreId: member.runtime_id,
      deviceId: null,
      online: !!member.online,
      limited: false,
      seenAt: member.heartbeat_at ?? null,
    })
  }
  for (const device of devices ?? []) {
    if (device.kind === PEER_KIND || device.direction === 'outbound') continue
    const same = rows.find((row) => !row.local && !row.deviceId && sameName(row.name, device.name))
    if (same) {
      same.deviceId = device.device_id
      same.seenAt ??= device.last_seen_at ?? null
      continue
    }
    const [kind, icon] = deviceKind(device.kind)
    rows.push({
      key: `device:${device.device_id}`,
      name: device.name || kindWord(kind),
      kind,
      icon,
      version: '',
      local: false,
      runtime: null,
      coreId: null,
      deviceId: device.device_id,
      online: null,
      limited: !(device.scopes ?? []).includes(ADMIN),
      seenAt: device.last_seen_at ?? null,
    })
  }
  if (phone?.pair_id) {
    rows.push({ key: `phone:${phone.pair_id}`, name: 'Phone', kind: 'phone', icon: 'smartphone', version: '', local: false, runtime: null, coreId: null, deviceId: null, pairId: phone.pair_id, online: null, limited: true, seenAt: phone.created_at ?? null })
  }
  for (const device of cloudDevices ?? []) {
    if (device.current || device.revoked_at) continue
    const [kind, icon] = deviceKind(String(device.platform ?? '').toLowerCase())
    const name = device.platform ? platformDisplayName(device.platform) : kindWord(kind)
    // The phone paired through the relay is the account's phone.
    const paired = kind === 'phone' ? rows.find((row) => row.pairId) : null
    if (paired) { paired.seenAt = device.last_active_at ?? paired.seenAt; continue }
    if (rows.some((row) => !row.local && sameName(row.name, name))) continue
    rows.push({ key: `cloud:${device.device_id}`, name, kind, icon, version: '', local: false, runtime: null, coreId: null, deviceId: null, online: null, limited: true, seenAt: device.last_active_at ?? null })
  }
  return rows
}

// The kind chip a row shows, or '' when the name already says it.
export function rowKind(row) {
  if (row.local) return ''
  const word = row.runtime ? kindLabel(row.runtime) : kindWord(row.kind)
  return word === row.name ? '' : word
}

// The machines this app reaches that keep a direct link to `coreId`, so
// removing it asks each of them to drop the link.
export function linkHolders(swarm, coreId) {
  return Object.entries(swarm?.views ?? {}).flatMap(([runtime, view]) => directLinks(view)
    .filter((member) => member.runtime_id === coreId)
    .map(() => runtime))
}

// Whether a row can be removed from here: this app reaches it, a machine it
// reaches links to it, or it holds access to this computer.
export function removable(row, swarm) {
  return !row.local && (!!row.runtime || !!row.deviceId || !!row.pairId || (!!row.coreId && linkHolders(swarm, row.coreId).length > 0))
}
