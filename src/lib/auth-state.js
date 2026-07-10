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
    message: `${labels[action]} — ${errorMessage(error)}. Try again.`,
    retry: action,
  }
}

export function errorKind(error) {
  return error && typeof error === 'object' && typeof error.kind === 'string'
    ? error.kind
    : 'internal'
}

export function errorMessage(error) {
  return error && typeof error === 'object' && typeof error.message === 'string'
    ? error.message
    : String(error)
}
