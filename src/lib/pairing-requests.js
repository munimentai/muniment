import { PAIRING_POLL_MS, PAIRING_REPLACE_MS } from './pairing-state.js'

function wait(ms, signal) {
  return new Promise((resolve, reject) => {
    const cancel = () => {
      clearTimeout(timer)
      reject(new DOMException('The pairing request stopped.', 'AbortError'))
    }
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', cancel)
      resolve()
    }, ms)
    signal.addEventListener('abort', cancel, { once: true })
    if (signal.aborted) cancel()
  })
}

// Keep request bounds across Settings closure and component remounts.
export function createPairingRequests() {
  let statusPending = false
  let statusAfter = -Infinity
  let challengePending = false
  let challengeAfter = -Infinity
  return {
    async status(invoke, signal) {
      while (true) {
        signal.throwIfAborted()
        if (!statusPending && Date.now() >= statusAfter) break
        await wait(Math.max(1, statusPending ? PAIRING_POLL_MS : statusAfter - Date.now()), signal)
      }
      statusPending = true
      try {
        return await invoke('auth_pairing_status')
      } finally {
        statusPending = false
        statusAfter = Date.now() + PAIRING_POLL_MS
      }
    },
    async challenge(invoke) {
      if (challengePending || Date.now() < challengeAfter) {
        throw new Error('Wait ten seconds before replacing the code.')
      }
      challengePending = true
      challengeAfter = Date.now() + PAIRING_REPLACE_MS
      try {
        return await invoke('auth_pairing_challenge')
      } finally {
        challengePending = false
        challengeAfter = Date.now() + PAIRING_REPLACE_MS
      }
    },
  }
}

export const pairingRequests = createPairingRequests()
