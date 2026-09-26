export const PAIRING_POLL_MS = 2000
export const PAIRING_REPLACE_MS = 10000

export const pairingIdleState = { name: 'idle' }

export function pairingLoadingState() {
  return { name: 'loading' }
}

export function pairingReadyState(status) {
  return { name: 'ready', pair: status?.pair ?? null }
}

export function pairingErrorState() {
  return { name: 'error' }
}

export function pairingChallengeState(challenge, requestedAt = Date.now()) {
  return {
    name: 'challenge',
    expiresAt: challenge.expires_at,
    qrSvg: challenge.qr_svg,
    requestedAt,
  }
}

export function challengeExpired(expiresAt, now = Date.now()) {
  const deadline = Date.parse(expiresAt)
  return Number.isFinite(deadline) && now >= deadline
}

export function remainingLabel(expiresAt, now = Date.now()) {
  const deadline = Date.parse(expiresAt)
  if (!Number.isFinite(deadline)) return '00:00'
  const remaining = Math.max(0, Math.floor((deadline - now) / 1000))
  const minutes = String(Math.floor(remaining / 60)).padStart(2, '0')
  const seconds = String(remaining % 60).padStart(2, '0')
  return `${minutes}:${seconds}`
}

export function replacementReady(requestedAt, now = Date.now()) {
  return now - requestedAt >= PAIRING_REPLACE_MS
}
