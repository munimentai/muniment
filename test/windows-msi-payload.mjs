import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { equalPayload, tree } from './e2e/runner/subscriptions.mjs'

export function assertMsiPayload(expanded, installed) {
  const executables = Object.keys(tree(expanded)).filter(file => path.basename(file) === 'muniment-desktop.exe')
  if (executables.length !== 1) throw new Error('The admin image must contain exactly one desktop executable.')
  equalPayload(tree(path.dirname(path.join(expanded, executables[0]))), tree(installed))
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  assertMsiPayload(process.argv[2], process.argv[3])
}
