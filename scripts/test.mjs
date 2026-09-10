import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const vitest = fileURLToPath(new URL('../node_modules/vitest/vitest.mjs', import.meta.url))
for (const args of [
  ['run', '--root', '.', '--exclude', 'test/e2e/**', '--exclude', '**/*.browser.test.js', ...process.argv.slice(2)],
  ['run', '--config', 'vitest.browser.config.js'],
]) {
  const result = spawnSync(process.execPath, [vitest, ...args], { stdio: 'inherit' })
  if (result.error) throw result.error
  if (result.status !== 0) process.exit(result.status ?? 1)
}
