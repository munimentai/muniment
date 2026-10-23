// The speech model install as a composer notice at the composer's right edge,
// over the Voice button. While the model downloads, one notice with a progress
// bar stays without a clock. Once the model is on disk it turns into the
// installed notice, and a failed download turns into its failure with Try again.
import { formatBytes, installStateWords } from './speech-install.js'

export const SPEECH_INSTALL_NOTICE = 'speech-install'

// The progress of an `installing` status. Before the total is known, the bar
// has no `max` and shows as indeterminate.
export function speechInstallProgress(status) {
  const max = status?.totalBytes > 0 ? status.totalBytes : 0
  const value = Math.max(0, Math.min(status?.completedBytes ?? 0, max))
  return { label: 'Speech model download progress', max, value, text: max ? `${formatBytes(value)} of ${formatBytes(max)}` : '' }
}

export function createSpeechInstallNotice({ notices, cancel, retry }) {
  const id = SPEECH_INSTALL_NOTICE
  const dismissLabel = 'Dismiss voice notice'
  let attempt = 0
  let downloading = false

  // Reads each install status. Only a download this view saw start earns the
  // installed or failed notice, so a status read on its own shows nothing.
  // `error` replaces the download line, such as a cancel that failed.
  function update(status, error = '') {
    const state = status?.state
    if (state === 'installing') {
      const fields = { text: error || 'Downloading the speech model.', alert: !!error, progress: speechInstallProgress(status) }
      if (downloading) { notices.update(id, fields); return }
      downloading = true
      attempt += 1
      notices.show({ id, cause: `installing ${attempt}`, ...fields, placement: 'end', persistent: true, action: { label: 'Cancel', run: cancel }, dismissLabel: 'Hide download progress' })
      return
    }
    if (!downloading) return
    downloading = false
    if (state === 'installed') notices.show({ id, cause: 'installed', text: installStateWords('installed'), placement: 'end', dismissLabel })
    else if (state === 'failed') notices.show({ id, cause: `failed ${attempt}`, text: status.message ?? 'The speech model could not be installed.', placement: 'end', action: { label: 'Try again', run: retry }, dismissLabel })
    else notices.withdraw(id)
  }

  // Dictation started, so the installed or failed line has done its job. A
  // download in progress keeps its notice.
  function clear() {
    if (!downloading) notices.withdraw(id)
  }

  return { update, clear }
}
