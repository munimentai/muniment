// Every node test runs against a throwaway home. A spawned runner, a probe
// script or os.homedir() then reads this root and never the user's
// ~/.muniment, and the state root override points there too.
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

// Cargo and rustup keep their real homes, so a test can still locate the
// muniment-core checkout that Cargo resolves.
for (const [variable, directory] of [['CARGO_HOME', '.cargo'], ['RUSTUP_HOME', '.rustup']]) {
  const real = path.join(os.homedir(), directory)
  if (!process.env[variable] && fs.existsSync(real)) process.env[variable] = real
}
const home = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-test-home-')))
process.env.HOME = home
process.env.USERPROFILE = home
process.env.MUNIMENT_STATE_DIR = path.join(home, '.muniment')
