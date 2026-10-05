import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, lstatSync, readFileSync, readdirSync, rmSync, symlinkSync, truncateSync, writeFileSync, existsSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { execFileSync, spawnSync } from 'node:child_process'
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
if (args[6] === 's3api' && args[7] === 'get-object') fs.writeFileSync(args[12], 'readback');
else if (args[8].startsWith('s3://')) {
  console.error('An error occurred (403) when calling the HeadObject operation: Forbidden');
  process.exit(1);
}
`, { mode: 0o755 })
  const env = { ...process.env, AWS_CLI: cli, AWS_ACCESS_KEY_ID: 'probe-access', AWS_SECRET_ACCESS_KEY: 'probe-secret', RECORD: record }
  const store = transport(env)
  store.put(prefix + 'file', Buffer.from('upload'))
  let args = JSON.parse(readFileSync(record))
  assert.deepEqual(args.slice(0, 8), ['--endpoint-url', endpoint, '--cli-connect-timeout', '15', '--cli-read-timeout', '120', 's3', 'cp'])
  assert.equal(endpoint, 'https://s3.roo.run')
  assert.equal(args.includes('probe-secret'), false)
  assert.equal(store.get(prefix + 'nested/file', 8).toString(), 'readback')
  args = JSON.parse(readFileSync(record))
  assert.deepEqual(args.slice(0, 12), ['--endpoint-url', endpoint, '--cli-connect-timeout', '15', '--cli-read-timeout', '120',
    's3api', 'get-object', '--bucket', 'factory-ci-artifacts', '--key', 'muniment-desktop/42/evidence/nested/file'])
  assert.equal(args.length, 13)
  assert.equal(existsSync(args[12]), false)
  assert.throws(() => store.get(prefix + 'file', 7), /size limit/)
  assert.throws(() => transport({ ...env, FAIL: '1' }).get(prefix + 'file', 100), /^Error: MinIO transfer failed: CLI exited with status 1\.$/)
  assert.throws(() => transport({ ...env, AWS_SECRET_ACCESS_KEY: '' }).get(prefix + 'file', 100), /Missing MinIO credentials/)
})

test('Uploads allow bounded transfer time for slow links without changing the read timeout.', () => {
  const env = { AWS_ACCESS_KEY_ID: 'test', AWS_SECRET_ACCESS_KEY: 'test' }
  for (const [size, expected] of [
    [0, 180_000], [1, 181_000], [256 * 1024, 181_000], [256 * 1024 + 1, 182_000],
    [64 * 1024 * 1024, 436_000], [256 * 1024 * 1024, 600_000],
  ]) {
    let call
    const store = transport(env, (_cli, args, options) => {
      call = { args, options, size: lstatSync(args[8]).size }
    })
    store.put(prefix + 'diagnostics.bin', Buffer.alloc(size))
    assert.equal(call.size, size)
    assert.equal(call.options.timeout, expected)
    assert.equal(call.options.stdio, 'pipe')
    assert.equal(call.options.maxBuffer, 4096)
    assert.deepEqual(call.args.slice(2, 6), ['--cli-connect-timeout', '15', '--cli-read-timeout', '120'])
    assert.equal(existsSync(call.args[8]), false)
  }
  const store = transport(env, (_cli, args, options) => {
    assert.equal(args[7], 'get-object')
    assert.equal(options.timeout, 180_000)
    writeFileSync(args[12], '')
  })
  assert.equal(store.get(prefix + 'diagnostics.bin', 64 * 1024 * 1024).length, 0)
})

test('An upload timeout still blocks the manifest and removes the temporary payload.', t => {
  const f = fixture(t)
  let file
  let calls = 0
  const store = transport({ AWS_ACCESS_KEY_ID: 'test', AWS_SECRET_ACCESS_KEY: 'test' }, (_cli, args) => {
    calls++
    assert.equal(args[7], 'cp')
    file = args[8]
    assert.equal(args[9], prefix + 'proof.json')
    throw Object.assign(new Error('private-message'), { code: 'ETIMEDOUT' })
  })
  assert.throws(() => upload(id, 'evidence', f.input, store), /^Error: MinIO transfer failed: timeout\.$/)
  assert.equal(calls, 1)
  assert.equal(existsSync(file), false)
})

test('Transport errors expose only a fixed class and a validated exit status.', () => {
  const env = { AWS_ACCESS_KEY_ID: 'private-access', AWS_SECRET_ACCESS_KEY: 'private-secret' }
  const cases = [
    [{ code: 'ENOENT' }, 'CLI missing'],
    [{ code: 'ETIMEDOUT', status: 1 }, 'timeout'],
    [{ code: 'ENOBUFS' }, 'CLI output exceeds the size limit'],
    [{ status: 1 }, 'CLI exited with status 1'],
    [{ status: 255 }, 'CLI exited with status 255'],
    ...[0, -1, 256, NaN, Infinity, 'private-status', null].map(status => [{ status }, 'CLI execution failed']),
    [{ code: 'private-code', signal: 'private-signal' }, 'CLI execution failed'],
  ]
  for (const [fields, message] of cases) {
    const execute = () => { throw Object.assign(new Error('private-message'), fields, {
      stdout: 'private-stdout', stderr: 'private-stderr', path: 'private-path', spawnargs: ['private-key'],
    }) }
    for (const operation of ['put', 'get']) {
      const store = transport(env, execute)
      assert.throws(() => store[operation](prefix + 'private-key', operation === 'put' ? Buffer.from('data') : 100), error => {
        assert.equal(error.message, `MinIO transfer failed: ${message}.`)
        assert.deepEqual(Object.getOwnPropertyNames(error).sort(), ['message', 'stack'])
        assert.doesNotMatch(error.stack, /private-/)
        return true
      })
    }
  }

  const store = transport(env, (_cli, _args, options) => {
    assert.equal(options.timeout, 180_000)
    assert.equal(options.stdio, 'pipe')
    execFileSync(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { ...options, timeout: 50 })
  })
  assert.throws(() => store.get(prefix + 'file', 100), /^Error: MinIO transfer failed: timeout\.$/)
})

test('S3 failures expose only bounded, allow-listed codes and operations.', () => {
  const env = { AWS_ACCESS_KEY_ID: 'private-access', AWS_SECRET_ACCESS_KEY: 'private-secret' }
  const diagnostic = (code, operation = 'CreateMultipartUpload') =>
    `upload failed: private-key to https://private-host/?X-Amz-Signature=private-signature\n` +
    `An error occurred (${code}) when calling the ${operation} operation: private-secret private-token`
  const cases = [
    [diagnostic('AccessDenied'), ' S3 error: AccessDenied (CreateMultipartUpload).'],
    [Buffer.from(diagnostic('EntityTooLarge', 'PutObject')), ' S3 error: EntityTooLarge (PutObject).'],
    [diagnostic('InvalidPart', 'CompleteMultipartUpload'), ' S3 error: InvalidPart (CompleteMultipartUpload).'],
    [diagnostic('NoSuchUpload', 'UploadPart'), ' S3 error: NoSuchUpload (UploadPart).'],
    [diagnostic('413', 'UploadPart'), ' S3 error: 413 (UploadPart).'],
    [diagnostic('1234567890'), ''],
    [diagnostic('AccessDenied', 'PrivateOperation'), ' S3 error: AccessDenied.'],
    [diagnostic('PrivateCode'), ''],
    [diagnostic('AccessDenied-private-secret'), ''],
    [diagnostic('AccessDenied\nprivate-secret'), ''],
    [diagnostic('AccessDenied', 'PutObject\nprivate-secret'), ''],
    ['private-secret AccessDenied CreateMultipartUpload', ''],
    ['x'.repeat(4096) + diagnostic('AccessDenied'), ''],
    [Buffer.from('x'.repeat(4096) + diagnostic('AccessDenied')), ''],
    [diagnostic('AccessDenied') + 'x'.repeat(100_000), ' S3 error: AccessDenied (CreateMultipartUpload).'],
    ['', ''], [undefined, ''], [null, ''], [42, ''],
  ]
  for (const [stderr, suffix] of cases) {
    for (const [fields, message] of [
      [{ status: 1 }, 'CLI exited with status 1.'],
      [{ code: 'ENOBUFS', status: null }, 'CLI output exceeds the size limit.'],
    ]) {
      for (const operation of ['put', 'get']) {
        const store = transport(env, () => {
          throw Object.assign(new Error('private-message'), fields, { stderr, stdout: 'private-stdout' })
        })
        assert.throws(() => store[operation](prefix + 'private-key', operation === 'put' ? Buffer.from('data') : 100), error => {
          assert.equal(error.message, `MinIO transfer failed: ${message}${suffix}`)
          assert.ok(error.message.length < 160)
          assert.deepEqual(Object.getOwnPropertyNames(error).sort(), ['message', 'stack'])
          assert.doesNotMatch(error.stack, /private-|Private|https:|X-Amz/)
          return true
        })
      }
    }
  }
})

test('The CLI reports redacted failure classes and keeps every failure blocked.', t => {
  const f = fixture(t)
  const cli = join(f.root, 'aws')
  writeFileSync(cli, `#!/usr/bin/env node
const fs = require('node:fs'); const path = require('node:path');
const args = process.argv.slice(2);
const [source, target] = args[6] === 's3api' && args[7] === 'get-object'
  ? ['s3://' + args[9] + '/' + args[11], args[12]] : args.slice(8, 10);
if (args[6] === 's3' && source.startsWith('s3://')) {
  console.error('An error occurred (403) when calling the HeadObject operation: Forbidden');
  process.exit(1);
}
if (process.env.FAIL) {
  console.log(process.env.AWS_ACCESS_KEY_ID, source, target);
  console.error(process.env.AWS_SECRET_ACCESS_KEY, process.env.AWS_SESSION_TOKEN);
  if (process.env.S3_FAIL) console.error('An error occurred (AccessDenied) when calling the CreateMultipartUpload operation: private-secret');
  process.exit(17);
}
const local = value => value.startsWith('s3://') ? path.join(process.env.STORE, value.slice(5)) : value;
fs.mkdirSync(path.dirname(local(target)), { recursive: true });
fs.copyFileSync(local(source), local(target));
if (source.startsWith('s3://')) {
  if (process.env.CORRUPT) {
    const bytes = fs.readFileSync(local(target)); bytes[0] ^= 1; fs.writeFileSync(local(target), bytes);
  }
  if (process.env.OVERSIZE) fs.appendFileSync(local(target), 'extra');
}
`, { mode: 0o755 })
  const env = { ...process.env, AWS_CLI: cli, AWS_ACCESS_KEY_ID: 'private-access',
    AWS_SECRET_ACCESS_KEY: 'private-secret', AWS_SESSION_TOKEN: 'private-token', STORE: join(f.root, 'store'),
    GITHUB_REPOSITORY: id.repository, GITHUB_RUN_ID: String(id.run), GITHUB_RUN_ATTEMPT: String(id.attempt), SOURCE_SHA: id.source }
  const run = (extra = {}, input = f.input) => spawnSync(process.execPath, ['.github/lib/artifact-store.mjs', 'upload', 'private-key', input], {
    env: { ...env, ...extra }, encoding: 'utf8', timeout: 10_000,
  })
  const cases = [
    [{ AWS_CLI: join(f.root, 'private-missing-cli') }, 'MinIO transfer failed: CLI missing.'],
    [{ FAIL: '1' }, 'MinIO transfer failed: CLI exited with status 17.'],
    [{ FAIL: '1', S3_FAIL: '1' }, 'MinIO transfer failed: CLI exited with status 17. S3 error: AccessDenied (CreateMultipartUpload).'],
    [{ CORRUPT: '1' }, 'Artifact readback mismatch.'],
    [{ OVERSIZE: '1' }, 'Artifact exceeds the size limit.'],
    [{ AWS_SECRET_ACCESS_KEY: '' }, 'Missing MinIO credentials.'],
  ]
  for (const [extra, message] of cases) {
    const result = run(extra)
    assert.equal(result.status, 1)
    assert.equal(result.stdout, '')
    assert.equal(result.stderr, message + '\n')
    assert.equal(existsSync(join(env.STORE, 'factory-ci-artifacts/muniment-desktop/42/private-key/manifest.json')), false)
  }
  const unknown = run({}, join(f.root, 'private-missing-input'))
  assert.equal(unknown.status, 1)
  assert.equal(unknown.stdout, '')
  assert.equal(unknown.stderr, 'The artifact transfer or integrity check failed.\n')

  truncateSync(join(f.input, 'proof.json'), 256 * 1024 * 1024 + 1)
  const oversized = run()
  assert.equal(oversized.status, 1)
  assert.equal(oversized.stdout, '')
  assert.equal(oversized.stderr, 'Artifact exceeds the size limit.\n')
})

test('Trusted CI proves upload, readback and collection without account credentials.', t => {
  const f = fixture(t)
  const cli = join(f.root, 'aws')
  writeFileSync(cli, `#!/usr/bin/env node
const fs = require('node:fs'); const path = require('node:path');
const args = process.argv.slice(2);
const [source, target] = args[6] === 's3api' && args[7] === 'get-object'
  ? ['s3://' + args[9] + '/' + args[11], args[12]] : args.slice(8, 10);
if (args[6] === 's3' && source.startsWith('s3://')) {
  console.error('An error occurred (403) when calling the HeadObject operation: Forbidden');
  process.exit(1);
}
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
  const objectRoot = join(env.STORE, 'factory-ci-artifacts/muniment-desktop/42')
  const uploaded = readFileSync(join(objectRoot, 'transport-probe-diagnostics/attempt-2/diagnostics.bin'))
  assert.equal(uploaded.length, 64 * 1024 * 1024)
  assert.ok(readFileSync(join(objectRoot, 'transport-probe-diagnostics-collection/attempt-2/diagnostics.bin')).equals(uploaded))
  assert.ok(readFileSync(join(f.root, 'collect/artifact-store-probe-2/diagnostics/attempt-2/diagnostics.bin')).equals(uploaded))
})

function checkArtifactSetup(text, label, stepIndent = 6) {
  const setupAction = 'uses: ./.github/actions/setup-artifact-store'
  // Reset setup at each job. Composite actions have one step list.
  for (const job of text.split(/^  (?=[\w-]+:\s*$)/m)) {
    let setup
    for (const step of job.split(new RegExp(`^ {${stepIndent}}- `, 'm')).slice(1)) {
      const condition = step.match(/^\s+if:\s*(.+)$/m)?.[1].trim()
      if (step.includes(setupAction)) {
        assert.doesNotMatch(step, /continue-on-error:\s*true/, label)
        setup = { condition }
      }
      if (!/\b(?:artifact-store(?:-probe)?\.mjs|publish-ci-artifacts\.sh)\b/.test(step)) continue
      assert.ok(setup, `${label}: Set up the artifact store before each call in the same job.`)
      assert.ok(setup.condition === 'always()' || setup.condition === condition,
        `${label}: Run setup under the same condition as the artifact call.`)
    }
  }
}

test('Every direct artifact call has setup in the same job before the call.', () => {
  for (const name of readdirSync('.github/workflows').filter(name => /\.ya?ml$/.test(name))) {
    checkArtifactSetup(readFileSync(`.github/workflows/${name}`, 'utf8'), name)
  }
  for (const name of readdirSync('.github/actions')) {
    const path = `.github/actions/${name}/action.yml`
    if (existsSync(path)) checkArtifactSetup(readFileSync(path, 'utf8'), path, 4)
  }
})

test('The workflow guard rejects missing, late and conditional setup in each E2E job.', () => {
  const nightly = readFileSync('.github/workflows/nightly.yml', 'utf8')
  const setup = '      - uses: ./.github/actions/setup-artifact-store\n        if: always()\n\n'
  for (const platform of ['linux', 'windows', 'macos']) {
    const start = nightly.indexOf(`  ${platform}-e2e:`)
    const index = nightly.indexOf(setup, start)
    const publish = nightly.indexOf('      - name: Publish diagnostics and JUnit to MinIO', start)
    assert.ok(index > start && index < publish)
    const before = nightly.slice(0, index)
    const after = nightly.slice(index + setup.length)
    assert.throws(() => checkArtifactSetup(before + after, platform), /Set up the artifact store/)
    assert.throws(() => checkArtifactSetup(before + setup.replace('        if: always()\n', '') + after, platform), /same condition/)
    const next = after.indexOf('      - name: Preserve E2E result')
    assert.throws(() => checkArtifactSetup(before + after.slice(0, next) + setup + after.slice(next), platform), /Set up the artifact store/)
  }
  const call = '      - run: node .github/lib/artifact-store.mjs upload evidence input\n'
  assert.throws(() => checkArtifactSetup(`jobs:\n  first:\n    steps:\n${call}${setup}`, 'late'), /Set up the artifact store/)
  assert.throws(() => checkArtifactSetup(`jobs:\n  first:\n    steps:\n${setup}  second:\n    steps:\n${call}`, 'other job'), /Set up the artifact store/)
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
