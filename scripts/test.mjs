import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { coreRoot } from './muniment-core.mjs'

if (process.allowedNodeEnvironmentFlags.has('--no-experimental-webstorage')) {
  process.env.NODE_OPTIONS = `${process.env.NODE_OPTIONS ?? ''} --no-experimental-webstorage`.trim()
}

// Resolve the muniment-core checkout once. The test processes inherit its path.
coreRoot()

const subscriptions = spawnSync(process.execPath, ['--test', 'test/subscription-acceptance.node.mjs'], { stdio: 'inherit' })
if (subscriptions.error) throw subscriptions.error
if (subscriptions.status !== 0) process.exit(subscriptions.status ?? 1)

const vitest = fileURLToPath(new URL('../node_modules/vitest/vitest.mjs', import.meta.url))
for (const args of [
  ['run', '--root', '.', '--exclude', 'test/e2e/**', '--exclude', 'prototypes/**', '--exclude', 'browser-control/**', '--exclude', '**/*.browser.test.js', ...process.argv.slice(2)],
  ['run', '--config', 'vitest.browser.config.js'],
]) {
  const result = spawnSync(process.execPath, [vitest, ...args], { stdio: 'inherit' })
  if (result.error) throw result.error
  if (result.status !== 0) process.exit(result.status ?? 1)
}
