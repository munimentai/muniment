// Every node test runs against a throwaway home. A spawned runner, a probe
// script or os.homedir() then reads this root and never the user's
// ~/.muniment, and the state root override points there too.
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const home = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-test-home-')))
process.env.HOME = home
process.env.USERPROFILE = home
process.env.MUNIMENT_STATE_DIR = path.join(home, '.muniment')
