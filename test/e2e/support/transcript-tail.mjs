import fs from 'node:fs'
import { pathToFileURL } from 'node:url'
import { diagnosticTail, subscriptionRedactor, transcriptText } from './subscription-diagnostics.mjs'

export function transcriptTail(text, redact = subscriptionRedactor({
  leases: process.env.FACTORY_SUBSCRIPTION_LEASES,
  values: [process.env.FIXTURE_USERNAME, process.env.MUNIMENT_E2E_USERNAME],
})) {
  // Redact before filtering or truncating so neither operation can split a secret.
  const meaningful = transcriptText(redact(text)).split('\n').filter(line => line.trim())
  return diagnosticTail(meaningful.slice(-200).join('\n'), redact)
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const tail = transcriptTail(fs.readFileSync(process.argv[2], 'utf8'))
    console.error(tail ? tail.split('\n').map(line => `  ${line}`).join('\n') : 'No diagnostic transcript lines remain.')
  } catch {
    console.error('The diagnostic transcript is unavailable.')
    process.exitCode = 1
  }
}
