const BACKOFF_SECONDS = [5, 10, 30, 60]

export const connectedState = { name: 'connected', failures: 0 }

export function enterUnreachable(previous, failure, now) {
  const failures = previous?.name === 'unreachable' ? previous.failures + 1 : 1
  const delay = BACKOFF_SECONDS[Math.min(failures - 1, BACKOFF_SECONDS.length - 1)]
  return {
    name: 'unreachable',
    failures,
    action: failure.action,
    message: failure.message,
    nextRetryAt: now + delay * 1000,
  }
}

export function secondsUntilRetry(state, now) {
  if (state.name !== 'unreachable') return 0
  return Math.max(0, Math.ceil((state.nextRetryAt - now) / 1000))
}

export function resetConnectivity() {
  return connectedState
}

export function diagnosticsText({ version, timestamp, action, issuerHost, message }) {
  return [
    `Muniment Desktop ${version}`,
    `Timestamp: ${new Date(timestamp).toISOString()}`,
    `Failed action: ${action}`,
    `Issuer host: ${issuerHost}`,
    `Error: ${message}`,
  ].join('\n')
}
