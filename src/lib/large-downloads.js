// Consent to large model downloads on this machine. An Install click beside
// the download size is the consent: it starts the download and core keeps
// the `large-downloads` preference, so the next large download starts
// without asking. A runtime that refuses the preference still installs on
// the click, and asks again next time.
import { agentRequest } from '../agents/agent-runtime.js'
import { LOCAL_RUNTIME_ID } from '../runtimes/runtimes.js'

export const LARGE_DOWNLOADS = 'large-downloads'

// Whether the user allowed large downloads on this machine before.
export async function downloadsAllowed(tauri) {
  try {
    const answer = await agentRequest(tauri, LOCAL_RUNTIME_ID, 'preferences.get', {})
    return answer?.values?.[LARGE_DOWNLOADS] === true
  } catch (_) {
    return false
  }
}

// Starts the speech model download with the user's consent and remembers it.
export function installSpeechModel(tauri) {
  const started = tauri.invoke('parakeet_install_start', { consent: true })
  void Promise.resolve()
    .then(() => agentRequest(tauri, LOCAL_RUNTIME_ID, 'preferences.set', { values: { [LARGE_DOWNLOADS]: true } }))
    .catch(() => {})
  return started
}

// Resumes a download the user allowed before, without asking.
export function resumeSpeechModel(tauri) {
  return tauri.invoke('parakeet_install_start', { consent: false })
}
