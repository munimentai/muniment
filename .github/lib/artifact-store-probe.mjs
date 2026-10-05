import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
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
// A collector rerun can depend on an upload job from an earlier attempt.
const uploadId = mode === 'upload' ? id : identity({
  ...process.env, GITHUB_RUN_ATTEMPT: process.env.UPLOAD_ATTEMPT,
})
assert.ok(Number.isSafeInteger(uploadId.attempt) && uploadId.attempt > 0 && uploadId.attempt <= id.attempt,
  'The upload attempt must be positive and cannot exceed the collector attempt.')
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
    download(uploadId, name, directory)
    assert.deepEqual(JSON.parse(readFileSync(join(directory, `${platform}-subscription.json`))), evidence)
    assert.deepEqual(JSON.parse(readFileSync(join(directory, 'release-acceptance.json'))), proof)
  }
}
// Use deterministic bytes without repeated blocks above the CLI multipart threshold.
const diagnostics = createHash('shake256', { outputLength: 64 * 1024 * 1024 })
  .update('muniment-artifact-store-diagnostics').digest()
const diagnosticsDirectory = join(root, 'diagnostics')
const diagnosticsPath = join(diagnosticsDirectory, `attempt-${uploadId.attempt}`, 'diagnostics.bin')
if (mode === 'upload') {
  mkdirSync(join(diagnosticsDirectory, `attempt-${id.attempt}`), { recursive: true, mode: 0o700 })
  writeFileSync(diagnosticsPath, diagnostics, { mode: 0o600 })
  upload(id, 'transport-probe-diagnostics', diagnosticsDirectory)
} else {
  download(uploadId, 'transport-probe-diagnostics', diagnosticsDirectory)
  assert.ok(readFileSync(diagnosticsPath).equals(diagnostics), 'The 64 MiB diagnostics must match byte-for-byte.')
  // Exercise large uploads and readbacks on the self-hosted collector too.
  upload(id, 'transport-probe-diagnostics-collection', diagnosticsDirectory)
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
