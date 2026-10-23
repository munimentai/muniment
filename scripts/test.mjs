import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

if (process.allowedNodeEnvironmentFlags.has('--no-experimental-webstorage')) {
  process.env.NODE_OPTIONS = `${process.env.NODE_OPTIONS ?? ''} --no-experimental-webstorage`.trim()
}

// CI stops at the first suite that fails. A local run goes on to every suite
// and exits with the first failure's status, so one failing suite never
// hides the others.
const stopEarly = !!process.env.CI
let failed = 0
function suite(args) {
  const result = spawnSync(process.execPath, args, { stdio: 'inherit' })
  if (result.error) throw result.error
  if (result.status === 0) return
  if (stopEarly) process.exit(result.status ?? 1)
  failed ||= result.status ?? 1
}

suite(['--test', 'test/subscription-acceptance.node.mjs'])

const vitest = fileURLToPath(new URL('../node_modules/vitest/vitest.mjs', import.meta.url))
for (const args of [
  ['run', '--root', '.', '--exclude', 'test/e2e/**', '--exclude', 'prototypes/**', '--exclude', 'browser-control/**', '--exclude', '**/*.browser.test.js', ...process.argv.slice(2)],
  ['run', '--config', 'vitest.browser.config.js'],
]) suite([vitest, ...args])
if (failed) process.exit(failed)
