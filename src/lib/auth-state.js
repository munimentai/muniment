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
    message: `${labels[action]} — ${String(error)}. Try again.`,
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
