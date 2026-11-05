// One folder holds every throwaway home a test run makes, and it goes when
// the run ends. A file whose tests all skip runs no afterAll, and a worker
// can end without an exit event, so this teardown is the one that never misses.
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

export default function setup() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-test-homes-'))
  process.env.MUNIMENT_TEST_HOMES = root
  return () => fs.rmSync(root, { recursive: true, force: true })
}
