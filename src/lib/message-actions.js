// The per-message action row (docs/spec/02-desktop-app.md §3.2). This slice
// carries copy only; fork, share, and retry wait on the thread/branching
// contract, so nothing here models them.

// Long enough to read the confirmation, short enough that the row never keeps
// making a claim about the clipboard that has stopped being the latest one.
export const COPY_CONFIRMATION_MS = 2000

// One slot for the whole thread: the newest attempt replaces the previous one,
// so two responses can never show a confirmation at the same time.
export function copyResult(runId, copied) {
  return { runId, status: copied ? 'copied' : 'failed' }
}

export function copyConfirmed(copy, runId) {
  return copy?.status === 'copied' && copy.runId === runId
}

export function copyLabel(copy, runId) {
  return copyConfirmed(copy, runId) ? 'Copied' : 'Copy'
}

// §1.7 error voice: what happened, then the next step, and no apology.
export function copyFailure(copy, runId, modifier = '⌘') {
  return copy?.status === 'failed' && copy.runId === runId
    ? `Clipboard unavailable. Select the reply and press ${modifier}C to copy it.`
    : ''
}

export function copyAnnouncement(copy, modifier = '⌘') {
  if (copy?.status === 'copied') return 'Reply copied to the clipboard.'
  return copyFailure(copy, copy?.runId, modifier)
}
