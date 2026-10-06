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

// Only fixed messages, allow-listed S3 codes and validated exit statuses reach the CLI log.
class ArtifactStoreError extends Error {}

const s3ErrorCodes = new Set([
  'AccessDenied', 'InvalidAccessKeyId', 'SignatureDoesNotMatch', 'ExpiredToken', 'InvalidToken',
  'EntityTooLarge', 'EntityTooSmall', 'InvalidPart', 'InvalidPartOrder', 'NoSuchUpload',
  'NoSuchBucket', 'NoSuchKey', 'RequestTimeout', 'SlowDown', 'InternalError', 'ServiceUnavailable',
  'NotImplemented', 'InvalidRequest', 'InvalidArgument', 'BadDigest', 'XAmzContentSHA256Mismatch',
  '400', '403', '404', '408', '413', '429', '500', '502', '503', '504',
])
const s3Operations = new Set([
  'PutObject', 'GetObject', 'HeadObject', 'CreateMultipartUpload', 'UploadPart',
  'CompleteMultipartUpload', 'AbortMultipartUpload', 'ListParts',
])

function s3ErrorClass(stderr) {
  const text = Buffer.isBuffer(stderr) ? stderr.subarray(0, 4096).toString('utf8') :
    typeof stderr === 'string' ? stderr.slice(0, 4096) : ''
  const match = text.match(/An error occurred \(([A-Za-z0-9]{1,64})\) when calling the ([A-Za-z0-9]{1,64}) operation/)
  if (!match || !s3ErrorCodes.has(match[1])) return ''
  const operation = s3Operations.has(match[2]) ? ` (${match[2]})` : ''
  return ` S3 error: ${match[1]}${operation}.`
}

function transferError(error) {
  let message = 'CLI execution failed.'
  if (error.code === 'ENOENT') message = 'CLI missing.'
  else if (error.code === 'ETIMEDOUT') message = 'timeout.'
  else if (error.code === 'ENOBUFS') message = 'CLI output exceeds the size limit.'
  else if (Number.isInteger(error.status) && error.status > 0 && error.status <= 255) {
    message = `CLI exited with status ${error.status}.`
  }
  return new ArtifactStoreError(`MinIO transfer failed: ${message}${s3ErrorClass(error.stderr)}`)
}

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
export function transport(env = process.env, execute = execFileSync) {
  const request = (args, timeout = 180_000) => {
    if (!env.AWS_ACCESS_KEY_ID || !env.AWS_SECRET_ACCESS_KEY) throw new ArtifactStoreError('Missing MinIO credentials.')
    try {
      execute(env.AWS_CLI || 'aws', ['--endpoint-url', endpoint,
        '--cli-connect-timeout', '15', '--cli-read-timeout', '120', ...args],
      { env: { ...env, AWS_DEFAULT_REGION: 'us-east-1', AWS_EC2_METADATA_DISABLED: 'true' },
        stdio: 'pipe', timeout, maxBuffer: 4096 })
    } catch (error) { throw transferError(error) }
  }
  return {
    put(key, bytes) {
      const work = mkdtempSync(join(tmpdir(), 'minio-put-'))
      try {
        const file = join(work, 'payload')
        writeFileSync(file, bytes, { mode: 0o600 })
        // Allow 256 KiB per second beyond setup time, with a ten-minute cap.
        const timeout = Math.min(600_000, 180_000 + Math.ceil(bytes.length / (256 * 1024)) * 1000)
        request(['s3', 'cp', file, key, '--only-show-errors'], timeout)
      } finally { rmSync(work, { recursive: true, force: true }) }
    },
    get(key, limit) {
      const work = mkdtempSync(join(tmpdir(), 'minio-get-'))
      try {
        const file = join(work, 'payload')
        const url = new URL(key)
        // GetObject reads the object without the HeadObject preflight that scoped credentials deny.
        request(['s3api', 'get-object', '--bucket', url.hostname, '--key', url.pathname.slice(1), file])
        if (lstatSync(file).size > limit) throw new ArtifactStoreError('Artifact exceeds the size limit.')
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
    } else if (stat.isFile() && safePath(name)) {
      if (stat.size > maxFile) throw new ArtifactStoreError('Artifact exceeds the size limit.')
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
  if (files.reduce((total, file) => total + file.bytes.length, 0) > maxFile) throw new ArtifactStoreError('Artifact exceeds the size limit.')
  const manifest = { schema: 1, repository: id.repository, run: id.run, attempt: id.attempt,
    source: id.source, name, files: [] }
  const publish = (key, bytes) => {
    for (let attempt = 1; ; attempt++) {
      store.put(key, bytes)
      try {
        if (!bytes.equals(store.get(key, bytes.length))) throw new ArtifactStoreError('Artifact readback mismatch.')
        return
      } catch (error) {
        // Retry an acknowledged upload only when its readback reports a missing object.
        if (attempt >= 3 || !(error instanceof ArtifactStoreError) ||
            !error.message.endsWith(' S3 error: NoSuchKey (GetObject).')) throw error
      }
    }
  }
  for (const { path, bytes } of files) {
    publish(root + path, bytes)
    manifest.files.push({ path, size: bytes.length, sha256: digest(bytes) })
  }
  // Publish the manifest last. Partial writes cannot produce a verified collection.
  const bytes = Buffer.from(JSON.stringify(manifest))
  if (bytes.length > maxManifest) throw new ArtifactStoreError('Artifact manifest exceeds the size limit.')
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
    if (bytes.length !== file.size || digest(bytes) !== file.sha256) throw new ArtifactStoreError('Artifact digest mismatch.')
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
  } catch (error) {
    console.error(error instanceof ArtifactStoreError ? error.message : 'The artifact transfer or integrity check failed.')
    process.exitCode = 1
  }
}
