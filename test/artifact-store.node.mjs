import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync, existsSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'
import { endpoint, upload, download, readArtifact, transport } from '../.github/lib/artifact-store.mjs'

const id = { repository: 'munimentai/muniment', run: 42, attempt: 2, source: 'a'.repeat(40) }
const prefix = 's3://factory-ci-artifacts/muniment-desktop/42/evidence/'
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'artifact-store-test-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const input = join(root, 'input')
  mkdirSync(input)
  writeFileSync(join(input, 'proof.json'), '{"passed":true}')
  const objects = new Map()
  const store = {
    put(key, bytes) { objects.set(key, Buffer.from(bytes)) },
    get(key, limit) {
      const bytes = objects.get(key)
      assert.ok(bytes, 'The object must exist.')
      assert.ok(bytes.length <= limit, 'The object must fit the limit.')
      return Buffer.from(bytes)
    },
  }
  const manifest = change => {
    const key = prefix + 'manifest.json'
    const value = JSON.parse(objects.get(key))
    change(value)
    objects.set(key, Buffer.from(JSON.stringify(value)))
  }
  return { root, input, objects, store, manifest }
}

test('The store verifies every upload and collects only verified files.', t => {
  const f = fixture(t)
  mkdirSync(join(f.input, 'nested'))
  writeFileSync(join(f.input, 'nested', 'empty.log'), '')
  const manifest = upload(id, 'evidence', f.input, f.store)
  assert.equal(manifest.files.length, 2)
  assert.equal(manifest.repository, id.repository)
  download(id, 'evidence', join(f.root, 'output'), f.store)
  assert.equal(readFileSync(join(f.root, 'output/proof.json'), 'utf8'), '{"passed":true}')
  assert.equal(readFileSync(join(f.root, 'output/nested/empty.log')).length, 0)
  assert.throws(() => download(id, 'evidence', join(f.root, 'output'), f.store))
})

test('The store rejects failed readbacks and partial uploads.', t => {
  const f = fixture(t)
  for (const get of [() => Buffer.from('corrupt'), () => { throw new Error('Offline.') }]) {
    assert.throws(() => upload(id, 'evidence', f.input, { ...f.store, get }))
    assert.equal(f.objects.has(prefix + 'manifest.json'), false)
  }
  assert.throws(() => readArtifact(id, 'evidence', f.store))
})

test('The store rejects empty directories, links and invalid identities.', t => {
  const f = fixture(t)
  for (const change of [{ repository: 'other/muniment' }, { run: 0 }, { run: -1 }, { run: 1.5 },
    { run: Number.MAX_SAFE_INTEGER + 1 }, { attempt: 0 }, { attempt: NaN }, { source: 'main' }, { source: '' }]) {
    assert.throws(() => upload({ ...id, ...change }, 'evidence', f.input, f.store))
  }
  for (const name of ['', '../escape', 'a/b', '.', '..']) {
    assert.throws(() => upload(id, name, f.input, f.store))
  }
  rmSync(join(f.input, 'proof.json'))
  assert.throws(() => upload(id, 'evidence', f.input, f.store))
  symlinkSync(join(f.root, 'secret'), join(f.input, 'proof.json'))
  assert.throws(() => upload(id, 'evidence', f.input, f.store))
})

test('The collector rejects missing, corrupt, stale and mismatched evidence without local residue.', t => {
  const f = fixture(t)
  const mutations = [
    m => { m.repository = 'other/muniment' }, m => { m.run++ }, m => { m.attempt-- },
    m => { m.source = 'b'.repeat(40) }, m => { m.name = 'other' }, m => { m.schema++ },
    m => { m.files = [] }, m => { m.files.push(m.files[0]) },
    m => { m.files[0].path = '../escape' }, m => { m.files[0].path = '/absolute' },
    m => { m.files[0].path = 'manifest.json' }, m => { m.files[0].path = 'a/../../escape' },
    m => { m.files[0].size = -1 }, m => { m.files[0].size = 0.5 },
    m => { m.files[0].size = 300 * 1024 * 1024 }, m => { m.files[0].sha256 = 'bad' },
  ]
  for (const mutate of mutations) {
    upload(id, 'evidence', f.input, f.store)
    f.manifest(mutate)
    assert.throws(() => download(id, 'evidence', join(f.root, 'output'), f.store))
    assert.equal(existsSync(join(f.root, 'output')), false)
  }
  for (const bytes of [undefined, Buffer.from('{"passed":null}'), Buffer.from('short')]) {
    upload(id, 'evidence', f.input, f.store)
    f.objects.set(prefix + 'proof.json', bytes)
    assert.throws(() => download(id, 'evidence', join(f.root, 'output'), f.store))
  }
  upload(id, 'evidence', f.input, f.store)
  f.objects.set(prefix + 'manifest.json', Buffer.from('{'))
  assert.throws(() => download(id, 'evidence', join(f.root, 'output'), f.store))
  assert.deepEqual(readdirSync(f.root), ['input'])
})

test('A failed rerun cannot reuse an earlier attempt.', t => {
  const f = fixture(t)
  upload(id, 'evidence', f.input, f.store)
  assert.throws(() => readArtifact({ ...id, attempt: 3 }, 'evidence', f.store))
  writeFileSync(join(f.input, 'proof.json'), 'changed')
  assert.throws(() => upload({ ...id, attempt: 3 }, 'evidence', f.input, {
    ...f.store, get: () => { throw new Error('Offline.') },
  }))
  assert.throws(() => readArtifact(id, 'evidence', f.store))
  assert.throws(() => readArtifact({ ...id, attempt: 3 }, 'evidence', f.store))
})

test('The CLI uses signed HTTPS without delete permission or credential output.', t => {
  const f = fixture(t)
  const cli = join(f.root, 'aws')
  const record = join(f.root, 'record.json')
  writeFileSync(cli, `#!/usr/bin/env node
const fs = require('node:fs');
const args = process.argv.slice(2);
fs.writeFileSync(process.env.RECORD, JSON.stringify(args));
if (process.env.FAIL) { console.error(process.env.AWS_SECRET_ACCESS_KEY); process.exit(1); }
const source = args[8], target = args[9];
if (source.startsWith('s3://')) fs.writeFileSync(target, 'readback');
`, { mode: 0o755 })
  const env = { ...process.env, AWS_CLI: cli, AWS_ACCESS_KEY_ID: 'probe-access', AWS_SECRET_ACCESS_KEY: 'probe-secret', RECORD: record }
  const store = transport(env)
  store.put(prefix + 'file', Buffer.from('upload'))
  let args = JSON.parse(readFileSync(record))
  assert.deepEqual(args.slice(0, 8), ['--endpoint-url', endpoint, '--cli-connect-timeout', '15', '--cli-read-timeout', '120', 's3', 'cp'])
  assert.equal(endpoint, 'https://s3.roo.run')
  assert.equal(args.includes('probe-secret'), false)
  assert.equal(store.get(prefix + 'file', 8).toString(), 'readback')
  assert.throws(() => store.get(prefix + 'file', 7), /size limit/)
  assert.throws(() => transport({ ...env, FAIL: '1' }).get(prefix + 'file', 100), /^Error: MinIO transfer failed\.$/)
  assert.throws(() => transport({ ...env, AWS_SECRET_ACCESS_KEY: '' }).get(prefix + 'file', 100), /Missing MinIO credentials/)
})

test('Trusted CI proves upload, readback and collection without account credentials.', t => {
  const f = fixture(t)
  const cli = join(f.root, 'aws')
  writeFileSync(cli, `#!/usr/bin/env node
const fs = require('node:fs'); const path = require('node:path');
const [source, target] = process.argv.slice(10, 12);
const local = value => value.startsWith('s3://') ? path.join(process.env.STORE, value.slice(5)) : value;
fs.mkdirSync(path.dirname(local(target)), { recursive: true });
fs.copyFileSync(local(source), local(target));
`, { mode: 0o755 })
  const env = { ...process.env, AWS_CLI: cli, AWS_ACCESS_KEY_ID: 'test', AWS_SECRET_ACCESS_KEY: 'test',
    STORE: join(f.root, 'store'), GITHUB_REPOSITORY: id.repository, GITHUB_RUN_ID: String(id.run),
    GITHUB_RUN_ATTEMPT: String(id.attempt), SOURCE_SHA: id.source }
  for (const mode of ['upload', 'collect']) {
    const temp = join(f.root, mode)
    mkdirSync(temp)
    const result = spawnSync(process.execPath, ['.github/lib/artifact-store-probe.mjs', mode], {
      env: { ...env, RUNNER_TEMP: temp }, encoding: 'utf8', timeout: 60_000,
    })
    assert.equal(result.status, 0, result.stderr)
  }
})

test('Every workflow uses MinIO while native tests and public distribution keep their channels.', () => {
  const workflows = readdirSync('.github/workflows').filter(name => name.endsWith('.yml'))
  for (const name of workflows) {
    const text = readFileSync(`.github/workflows/${name}`, 'utf8')
    assert.doesNotMatch(text, /actions\/(?:upload|download)-artifact|listWorkflowRunArtifacts|downloadArtifact/)
  }
  const subscriptions = readFileSync('.github/workflows/subscriptions.yml', 'utf8')
  for (const platform of ['linux', 'windows', 'macos-arm64', 'macos-x64']) {
    assert.ok(subscriptions.includes(`name: subscription-${platform}`))
    assert.ok(subscriptions.includes(`path: \${{ runner.temp }}/subscription-${platform}`))
  }
  assert.match(subscriptions, /runs-on: macos-15/)
  assert.match(subscriptions, /node test\/e2e\/runner\/subscription-macos-arm64.mjs/)
  assert.equal((subscriptions.match(/uses: .\/.github\/actions\/store-artifact/g) ?? []).length, 5)
  assert.equal((subscriptions.match(/source: \$\{\{ inputs.source_sha \}\}/g) ?? []).length, 5)
  assert.match(subscriptions, /artifact-store.mjs download/)
  assert.match(subscriptions, /SUBSCRIPTION_JOB_RESULTS: \$\{\{ toJSON\(needs\) \}\}/)
  assert.match(subscriptions, /test "\$COLLECT_STATUS" = 0/)
  for (const step of subscriptions.split('      - ').filter(value => value.includes('run: node test/e2e/runner/subscription-'))) {
    assert.doesNotMatch(step, /FACTORY_CI_S3|AWS_SECRET_ACCESS_KEY/)
  }
  for (const name of ['ci', 'secret-scan', 'nightly']) {
    const text = readFileSync(`.github/workflows/${name}.yml`, 'utf8')
    assert.match(text, /uses: .\/.github\/actions\/store-artifact/)
    assert.match(text, /uses: .\/.github\/actions\/setup-artifact-store/)
    assert.ok(text.includes(`name: ${name}.yml-proof`))
    assert.match(text, /secrets.FACTORY_CI_S3_ACCESS_KEY/)
    assert.match(text, /secrets.FACTORY_CI_S3_SECRET_KEY/)
  }
  const proof = readFileSync('.github/lib/ci-proof.mjs', 'utf8')
  assert.match(proof, /attempt: run.run_attempt, source: run.head_sha/)
  assert.doesNotMatch(proof, /listWorkflowRunArtifacts|downloadArtifact/)
  const nightly = readFileSync('.github/workflows/nightly.yml', 'utf8')
  assert.match(nightly, /node .github\/upload-nightly-assets.mjs/)
  assert.match(readFileSync('.github/upload-nightly-assets.mjs', 'utf8'), /https:\/\/uploads.github.com\/repos\/\$\{repository\}\/releases\//)
  const probe = readFileSync('.github/workflows/artifact-store.yml', 'utf8')
  assert.match(probe, /head.repo.full_name == github.repository/)
  assert.match(probe, /runs-on: macos-15/)
  assert.match(probe, /artifact-store-probe.mjs upload/)
  assert.match(probe, /artifact-store-probe.mjs collect/)
  assert.doesNotMatch(probe, /SUBSCRIPTION_LEASES|continue-on-error/)
})
