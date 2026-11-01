// Every node test runs against a throwaway home. A spawned runner, a probe
// script or os.homedir() then reads this root and never the user's
// ~/.muniment, and the state root override points there too. The home goes
// when the test file ends, and test/isolated-home.global.js removes the
// folder that holds them all when the run ends.
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { afterAll } from 'vitest'

// Cargo and rustup keep their real homes, so a test can still run Cargo.
for (const [variable, directory] of [['CARGO_HOME', '.cargo'], ['RUSTUP_HOME', '.rustup']]) {
  const real = path.join(os.homedir(), directory)
  if (!process.env[variable] && fs.existsSync(real)) process.env[variable] = real
}
const homes = process.env.MUNIMENT_TEST_HOMES || os.tmpdir()
const home = fs.realpathSync.native(fs.mkdtempSync(path.join(homes, 'muniment-test-home-')))
process.env.HOME = home
process.env.USERPROFILE = home
process.env.MUNIMENT_STATE_DIR = path.join(home, '.muniment')

// The run's global teardown removes the folder that holds every home.
afterAll(() => {
  fs.rmSync(home, { recursive: true, force: true })
})
