import { spawnSync } from 'node:child_process'

// These fixtures cover the optional cloud and record surfaces as well as the shell.
const env = { ...process.env, VITE_MUNIMENT_CLOUD: 'true', VITE_MUNIMENT_COMPANY_RECORD: 'true' }
const build = spawnSync(process.execPath, ['node_modules/vite/bin/vite.js', 'build'], { env, stdio: 'inherit' })
if (build.status !== 0) process.exit(build.status || 1)
const check = spawnSync(process.execPath, ['test/probe/check.mjs'], { stdio: 'inherit' })
process.exit(check.status || (check.error ? 1 : 0))
