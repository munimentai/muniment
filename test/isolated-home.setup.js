// Every node test runs against a throwaway home. A spawned runner, a probe
// script or os.homedir() then reads this root and never the user's
// ~/.muniment, and the state root override points there too. The home goes
// when the test file ends, so test runs leave nothing in the temporary folder.
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { afterAll } from 'vitest'

// Cargo and rustup keep their real homes, so a test can still run Cargo.
for (const [variable, directory] of [['CARGO_HOME', '.cargo'], ['RUSTUP_HOME', '.rustup']]) {
  const real = path.join(os.homedir(), directory)
  if (!process.env[variable] && fs.existsSync(real)) process.env[variable] = real
}
const home = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-test-home-')))
process.env.HOME = home
process.env.USERPROFILE = home
process.env.MUNIMENT_STATE_DIR = path.join(home, '.muniment')

// A file that fails before its tests run skips afterAll, so the worker also
// removes every home it made when it exits.
const remove = (directory) => fs.rmSync(directory, { recursive: true, force: true })
if (!globalThis.munimentTestHomes) {
  globalThis.munimentTestHomes = new Set()
  process.once('exit', () => globalThis.munimentTestHomes.forEach(remove))
}
globalThis.munimentTestHomes.add(home)
afterAll(() => {
  remove(home)
  globalThis.munimentTestHomes.delete(home)
})
