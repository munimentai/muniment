import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import { pathToFileURL } from 'node:url'
import { SIGNING_LINE_PREFIX, defaultEnvironmentFile } from '../../../.github/lib/signing-env.mjs'

// The runners take the candidate from a release lookup by default. The pr mode
// takes it from the checkout under test, so the mode is explicit and fails closed.
export const SOURCE_MODES = ['nightly', 'pr']

export function resolveSourceMode(value) {
  if (value === undefined || value === '') return 'nightly'
  if (!SOURCE_MODES.includes(value)) throw new Error('invalid source mode')
  return value
}

export function checkoutHead(run = execFileSync) {
  let head
  try {
    head = String(run('git', ['rev-parse', 'HEAD'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] })).trim()
  } catch {
    throw new Error('the checkout source SHA is unavailable')
  }
  if (!/^[0-9a-f]{40}$/.test(head)) throw new Error('the checkout source SHA is unavailable')
  return head
}

export function verifySourceSha(expected, head = checkoutHead()) {
  if (!/^[0-9a-f]{40}$/.test(expected || '')) throw new Error('invalid source SHA')
  if (expected !== head) throw new Error(`source SHA ${expected} does not match checkout ${head}`)
  return head
}

// The pr mode builds unsigned packages. Signing material in the job means the
// job was configured for a release build, so the runner stops before the build
// helpers can take it.
export function refuseSigningMaterial(env = process.env, file = defaultEnvironmentFile()) {
  if (env.MACOS_SIGNING_ENABLED === 'true') throw new Error('the pr mode refuses signing')
  if (fs.existsSync(file) && fs.readFileSync(file, 'utf8').split(/\r?\n/).some((line) => line.startsWith(SIGNING_LINE_PREFIX))) {
    throw new Error('the pr mode refuses signing credentials')
  }
}

function main([command, file]) {
  if (command === 'mode') process.stdout.write(resolveSourceMode(process.env.MUNIMENT_E2E_SOURCE_MODE))
  else if (command === 'verify') verifySourceSha(process.env.MUNIMENT_E2E_SOURCE_SHA)
  else if (command === 'refuse-signing') refuseSigningMaterial(process.env, file)
  else throw new Error('usage: pr-source.mjs <mode|verify|refuse-signing>')
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main(process.argv.slice(2))
  } catch (error) {
    console.error(error.message)
    process.exit(1)
  }
}
