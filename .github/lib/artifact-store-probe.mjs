import assert from 'node:assert/strict'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { identity, upload, download } from './artifact-store.mjs'
import { blocked, platforms } from '../../test/e2e/support/subscription-acceptance.mjs'
import { collect } from '../../test/e2e/runner/collect-subscriptions.mjs'

// Transport fixtures never claim that an installed subscription check passed.
const reason = 'This transport probe has no subscription account credentials.'
const id = identity()
const root = join(process.env.RUNNER_TEMP, `artifact-store-probe-${id.attempt}`)
const mode = process.argv[2]
assert.ok(['upload', 'collect'].includes(mode))
const inputs = platforms.map(platform => join(root, platform))
for (const [index, platform] of platforms.entries()) {
  const directory = inputs[index]
  const evidence = { status: 'blocked', reason }
  const proof = blocked(id.source, platform, reason)
  const name = `transport-probe-${platform}`
  if (mode === 'upload') {
    mkdirSync(directory, { recursive: true, mode: 0o700 })
    writeFileSync(join(directory, `${platform}-subscription.json`), JSON.stringify(evidence))
    writeFileSync(join(directory, 'release-acceptance.json'), JSON.stringify(proof))
    upload(id, name, directory)
  } else {
    download(id, name, directory)
    assert.deepEqual(JSON.parse(readFileSync(join(directory, `${platform}-subscription.json`))), evidence)
    assert.deepEqual(JSON.parse(readFileSync(join(directory, 'release-acceptance.json'))), proof)
  }
}
if (mode === 'collect') {
  const output = join(root, 'collection')
  assert.equal(collect(id.source, output, inputs), 1)
  const expected = { schema: 1, source_sha: id.source, packages: {},
    cases: platforms.flatMap(platform => blocked(id.source, platform, reason).cases) }
  assert.deepEqual(JSON.parse(readFileSync(join(output, 'release-acceptance.json'))), expected)
  upload(id, 'transport-probe-collection', output)
  const readback = join(root, 'readback')
  download(id, 'transport-probe-collection', readback)
  assert.deepEqual(JSON.parse(readFileSync(join(readback, 'release-acceptance.json'))), expected)
}
console.log(`The MinIO ${mode} probe passed.`)
