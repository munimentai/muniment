import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

if (!process.env.PI_TEST_BINARY || !process.env.PI_TEST_PACKAGES) {
  console.error('Set PI_TEST_BINARY to the verified Pi executable and PI_TEST_PACKAGES to an isolated frozen package install.')
  process.exit(1)
}
const result = spawnSync(process.execPath, [fileURLToPath(new URL('../node_modules/vitest/vitest.mjs', import.meta.url)), 'run', '--root', '.', 'test/agent-dependencies.test.js', 'test/pi-runtime-compat.test.js'], { stdio: 'inherit' })
if (result.error) throw result.error
process.exit(result.status ?? 1)
