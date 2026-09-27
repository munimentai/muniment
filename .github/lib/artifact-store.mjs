import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs'
import { basename, dirname, join, resolve } from 'node:path'
import { tmpdir } from 'node:os'
import { pathToFileURL } from 'node:url'

export const endpoint = 'https://s3.roo.run'
const maxFile = 256 * 1024 * 1024
const maxManifest = 1024 * 1024
const digest = bytes => createHash('sha256').update(bytes).digest('hex')
const component = value => typeof value === 'string' && /^[a-zA-Z0-9][a-zA-Z0-9_.-]*$/.test(value)
const positive = value => Number.isSafeInteger(value) && value > 0

export function identity(env = process.env) {
  const number = value => /^[1-9][0-9]*$/.test(value ?? '') ? Number(value) : NaN
  return { repository: env.GITHUB_REPOSITORY, run: number(env.GITHUB_RUN_ID),
    attempt: number(env.GITHUB_RUN_ATTEMPT), source: env.SOURCE_SHA }
}

function prefix(id, name) {
  if (id.repository !== 'munimentai/muniment' || !positive(id.run) || !positive(id.attempt) ||
      !/^[a-f0-9]{40}$/.test(id.source ?? '') || !component(name)) throw new Error('Invalid artifact identity.')
  // Keep the factory release reader's run-scoped paths. The manifest binds the full identity.
  return `s3://factory-ci-artifacts/muniment-desktop/${id.run}/${name}/`
}

function safePath(path) {
  return typeof path === 'string' && path.split('/').every(part => component(part) && part !== '.' && part !== '..') &&
    path !== 'manifest.json'
}

// The CLI signs each request. Never print its output or pass credentials as arguments.
export function transport(env = process.env) {
  const copy = (source, target) => {
    if (!env.AWS_ACCESS_KEY_ID || !env.AWS_SECRET_ACCESS_KEY) throw new Error('Missing MinIO credentials.')
    try {
      execFileSync(env.AWS_CLI || 'aws', ['--endpoint-url', endpoint,
        '--cli-connect-timeout', '15', '--cli-read-timeout', '120', 's3', 'cp', source, target, '--only-show-errors'],
      { env: { ...env, AWS_DEFAULT_REGION: 'us-east-1', AWS_EC2_METADATA_DISABLED: 'true' },
        stdio: 'pipe', timeout: 180_000, maxBuffer: 4096 })
    } catch { throw new Error('MinIO transfer failed.') }
  }
  return {
    put(key, bytes) {
      const work = mkdtempSync(join(tmpdir(), 'minio-put-'))
      try {
        const file = join(work, 'payload')
        writeFileSync(file, bytes, { mode: 0o600 })
        copy(file, key)
      } finally { rmSync(work, { recursive: true, force: true }) }
    },
    get(key, limit) {
      const work = mkdtempSync(join(tmpdir(), 'minio-get-'))
      try {
        const file = join(work, 'payload')
        copy(key, file)
        if (lstatSync(file).size > limit) throw new Error('Artifact exceeds the size limit.')
        return readFileSync(file)
      } finally { rmSync(work, { recursive: true, force: true }) }
    },
  }
}

function filesAt(input) {
  const files = []
  function visit(file, name) {
    const stat = lstatSync(file)
    if (stat.isSymbolicLink()) throw new Error('Artifact links are not allowed.')
    if (stat.isDirectory()) {
      for (const child of readdirSync(file).sort()) visit(join(file, child), name ? `${name}/${child}` : child)
    } else if (stat.isFile() && safePath(name) && stat.size <= maxFile) {
      files.push({ path: name, bytes: readFileSync(file) })
    } else { throw new Error('Invalid artifact file.') }
  }
  visit(input, lstatSync(input).isDirectory() ? '' : basename(input))
  if (!files.length || files.length > 1000) throw new Error('Invalid artifact file count.')
  return files
}

export function upload(id, name, input, store = transport()) {
  const root = prefix(id, name)
  const files = filesAt(input)
  if (files.reduce((total, file) => total + file.bytes.length, 0) > maxFile) throw new Error('Artifact exceeds the size limit.')
  const manifest = { schema: 1, repository: id.repository, run: id.run, attempt: id.attempt,
    source: id.source, name, files: [] }
  const publish = (key, bytes) => {
    store.put(key, bytes)
    if (!bytes.equals(store.get(key, bytes.length))) throw new Error('Artifact readback mismatch.')
  }
  for (const { path, bytes } of files) {
    publish(root + path, bytes)
    manifest.files.push({ path, size: bytes.length, sha256: digest(bytes) })
  }
  // Publish the manifest last. Partial writes cannot produce a verified collection.
  const bytes = Buffer.from(JSON.stringify(manifest))
  if (bytes.length > maxManifest) throw new Error('Artifact manifest exceeds the size limit.')
  publish(root + 'manifest.json', bytes)
  return manifest
}

export function readArtifact(id, name, store = transport(), limit = maxFile) {
  const root = prefix(id, name)
  const manifest = JSON.parse(store.get(root + 'manifest.json', maxManifest))
  if (manifest.schema !== 1 || manifest.name !== name ||
      !['repository', 'run', 'attempt', 'source'].every(key => manifest[key] === id[key]) ||
      !Array.isArray(manifest.files) || !manifest.files.length || manifest.files.length > 1000) {
    throw new Error('Artifact identity mismatch.')
  }
  const files = new Map()
  let total = 0
  for (const file of manifest.files) {
    if (!safePath(file.path) || files.has(file.path) || !Number.isSafeInteger(file.size) || file.size < 0 ||
        file.size > limit || !/^[a-f0-9]{64}$/.test(file.sha256 ?? '') || (total += file.size) > maxFile) {
      throw new Error('Invalid artifact manifest.')
    }
    const bytes = store.get(root + file.path, file.size)
    if (bytes.length !== file.size || digest(bytes) !== file.sha256) throw new Error('Artifact digest mismatch.')
    files.set(file.path, bytes)
  }
  return files
}

export function download(id, name, output, store = transport()) {
  // Use a fresh destination. Never mix verified files with stale local evidence.
  if (lstatExists(output)) throw new Error('The artifact destination must not exist.')
  const files = readArtifact(id, name, store)
  mkdirSync(dirname(resolve(output)), { recursive: true, mode: 0o700 })
  const work = mkdtempSync(join(dirname(resolve(output)), '.artifact-'))
  try {
    for (const [path, bytes] of files) {
      const target = join(work, path)
      mkdirSync(dirname(target), { recursive: true, mode: 0o700 })
      writeFileSync(target, bytes, { mode: 0o600 })
    }
    renameSync(work, output)
  } finally { rmSync(work, { recursive: true, force: true }) }
}

function lstatExists(path) {
  try { lstatSync(path); return true } catch (error) { if (error.code === 'ENOENT') return false; throw error }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const [mode, name, path] = process.argv.slice(2)
    if (!path || !['upload', 'download'].includes(mode)) throw new Error('Use upload or download with an artifact name and path.')
    if (mode === 'upload') upload(identity(), name, path)
    else download(identity(), name, path)
    console.log(`Verified artifact: ${name}.`)
  } catch {
    console.error('The artifact transfer or integrity check failed.')
    process.exitCode = 1
  }
}
