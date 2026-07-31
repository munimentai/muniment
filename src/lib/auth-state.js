export const bootState = { name: 'boot' }

export function waitingState() {
  return { name: 'signing-in' }
}

export function statusState(status) {
  return status.signed_in
    ? { name: 'signed-in', subject: status.subject ?? 'Unknown subject' }
    : { name: 'signed-out' }
}

export function errorState(action, error) {
  const labels = {
    status: 'Session status unavailable',
    'sign-in': 'Sign-in not completed',
    'sign-out': 'Sign-out not completed',
  }

  return {
    name: 'error',
    message: `${labels[action]} — ${String(error)}.`,
    retry: action,
  }
}

export const accessIdleState = { name: 'idle' }

export function accessLoadingState() {
  return { name: 'loading' }
}

export function accessReadyState(snapshot) {
  return {
    name: 'ready',
    snapshot,
    groups: snapshot.groups.map((group) => ({
      ...group,
      models: group.models ?? [],
      connections: group.connections ?? [],
      capabilities: group.capabilities ?? [],
    })),
  }
}

export function accessErrorState(error) {
  return { name: 'error', message: String(error || 'Your access could not be loaded.') }
}

export const devicesIdleState = { name: 'idle' }

export function devicesLoadingState() {
  return { name: 'loading' }
}

export function platformDisplayName(platform) {
  const id = String(platform ?? '')
  const names = {
    macos: 'macOS',
    ios: 'iOS',
    windows: 'Windows',
    linux: 'Linux',
    android: 'Android',
    desktop: 'Desktop',
  }

  return Object.hasOwn(names, id) ? names[id] : `${id.charAt(0).toUpperCase()}${id.slice(1)}`
}

export function devicesReadyState(devices) {
  return {
    name: 'ready',
    devices: devices.map((device) => ({
      ...device,
      platform: platformDisplayName(device.platform),
    })).sort((a, b) => {
      const revoked = Number(Boolean(a.revoked_at)) - Number(Boolean(b.revoked_at))
      return revoked || Date.parse(b.last_active_at) - Date.parse(a.last_active_at) || a.device_id.localeCompare(b.device_id)
    }),
  }
}

export function devicesErrorState() {
  return { name: 'error' }
}
