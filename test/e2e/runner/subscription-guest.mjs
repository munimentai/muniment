import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'
import { createHash } from 'node:crypto'
import { platforms, subscriptionAccounts } from '../support/subscription-acceptance.mjs'
import { verifyUpdaterSignature } from '../../../.github/lib/updater-signature.mjs'
import { run, writeBlocked } from './subscriptions.mjs'
import { subscriptionRedactor, nativeFailure, readDiagnosticLog } from '../support/subscription-diagnostics.mjs'
import { prepareLinuxSandbox, sandboxRequirement } from './subscription-linux-sandbox.mjs'

const missingPackage = 'The signed nightly package or updater signature for this platform is missing.'
const missingLeases = 'Provide the FACTORY_SUBSCRIPTION_LEASES secret with access-only factory leases.'
const missingModels = 'Provide the FACTORY_SUBSCRIPTION_MODELS variable with four distinct supported model IDs.'

export function decodeSubscriptionPayload(encoded, reason) {
  if (typeof encoded !== 'string' || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(encoded)) {
    throw new Error(reason)
  }
  const bytes = Buffer.from(encoded, 'base64')
  if (bytes.toString('base64') !== encoded) throw new Error(reason)
  return bytes.toString('utf8')
}

function packagePattern(sourceSha, platform) {
  if (platform === 'linux') return new RegExp(`^nightly-${sourceSha}-linux-muniment_[0-9]+\\.[0-9]+\\.[0-9]+_amd64\\.AppImage$`)
  if (platform === 'windows') return new RegExp(`^nightly-${sourceSha}-windows-muniment_[0-9]+\\.[0-9]+\\.[0-9]+_x64_en-US\\.msi$`)
  if (platform === 'macos-arm64') return new RegExp(`^nightly-${sourceSha}-macos-muniment-arm64\\.app\\.tar\\.gz$`)
  if (platform === 'macos-x64') return new RegExp(`^nightly-${sourceSha}-macos-muniment-x64\\.app\\.tar\\.gz$`)
  throw new Error('Run this check on the requested native platform and architecture.')
}

// desktop-ci collects this default directory on each native runner.
export function defaultArtifactsDir(env = process.env, runtime = process.platform) {
  if (env.DCI_ARTIFACTS_DIR) return env.DCI_ARTIFACTS_DIR
  if (runtime === 'win32') return path.join(env.TEMP || env.TMP || os.tmpdir(), 'dci-artifacts')
  return '/tmp/dci-artifacts'
}

async function githubFetch(url, token, accept, timeout) {
  const response = await fetch(url, {
    headers: { Authorization: `Bearer ${token}`, Accept: accept },
    signal: AbortSignal.timeout(timeout),
  })
  if (!response.ok) throw new Error(missingPackage)
  return response
}

async function loadRelease(repository, token) {
  const response = await githubFetch(
    `https://api.github.com/repos/${repository}/releases/tags/nightly`,
    token, 'application/vnd.github+json', 60_000)
  return response.json()
}

async function downloadAsset(repository, token, assetId) {
  const response = await githubFetch(
    `https://api.github.com/repos/${repository}/releases/assets/${assetId}`,
    token, 'application/octet-stream', 180_000)
  return Buffer.from(await response.arrayBuffer())
}

export function selectAssets(release, sourceSha, platform) {
  const matches = (release.assets ?? []).filter(asset => packagePattern(sourceSha, platform).test(asset.name))
  if (matches.length !== 1 || !Number.isSafeInteger(matches[0].id) || matches[0].id <= 0) throw new Error(missingPackage)
  const signature = (release.assets ?? []).find(asset => asset.name === `${matches[0].name}.sig`)
  if (!signature || !Number.isSafeInteger(signature.id) || signature.id <= 0) throw new Error(missingPackage)
  return { packageAsset: matches[0], signatureAsset: signature }
}

function install(platform, packageFile, root) {
  if (platform === 'linux') {
    fs.chmodSync(packageFile, 0o755)
    return packageFile
  }
  if (platform.startsWith('macos-')) {
    const expanded = path.join(root, 'app')
    fs.mkdirSync(expanded)
    const result = spawnSync('tar', ['-xzf', packageFile, '-C', expanded], { timeout: 60_000 })
    if (result.error || result.status !== 0) throw nativeFailure('tar', result)
    const executable = path.join(expanded, 'muniment.app', 'Contents', 'MacOS', 'muniment-desktop')
    if (!fs.existsSync(executable)) throw new Error(missingPackage)
    return executable
  }
  const result = spawnSync('msiexec.exe', ['/i', packageFile, '/qn', '/norestart', '/L*v', path.join(root, 'install.log')], { timeout: 180_000, windowsHide: true })
  if (result.error || (result.status !== 0 && result.status !== 3010)) throw nativeFailure('msiexec/install', result)
  const executable = path.join(process.env.LOCALAPPDATA, 'muniment', 'muniment-desktop.exe')
  if (!fs.existsSync(executable)) throw new Error(missingPackage)
  return executable
}

// Keep one unlocked Keychain across chat, restart, and signed update checks.
export function runMacosProbe({ candidateFile, packageFile, signatureFile, executable, leasesFile, output, sourceSha, platform }, execute = spawnSync) {
  const result = execute('/bin/bash', [path.resolve('test/e2e/support/macos-keychain-session.sh'),
    process.execPath, path.resolve('test/e2e/runner/subscriptions.mjs'),
    candidateFile, packageFile, signatureFile, executable, leasesFile, output, sourceSha, platform],
  { timeout: 30 * 60_000, encoding: 'utf8', stdio: 'pipe' })
  if (result.error || result.status !== 0) throw nativeFailure('macos-keychain-session', result)
  return 0
}

export async function guest({ sourceSha, platform, output, leases, models, encodedLeases, encodedModels, repository, token, fetchRelease, fetchAsset, env = process.env, runtime = process.platform }) {
  const artifacts = output || defaultArtifactsDir(env, runtime)
  if (!platforms.includes(platform) || !/^[a-f0-9]{40}$/.test(sourceSha ?? '') || !artifacts) {
    throw new Error('Provide a supported native platform, output directory, and the exact candidate source SHA.')
  }
  fs.rmSync(path.join(artifacts, `${platform}-subscription.log`), { force: true })
  writeBlocked(artifacts, sourceSha, platform, 'Provide the pinned signed package, a native GUI runner, and fresh factory subscription access leases.')
  let step = 'guest/leases'
  let cleanupSandbox
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-guest-'))
  try {
    if (encodedLeases !== undefined) leases = decodeSubscriptionPayload(encodedLeases, missingLeases)
    if (encodedModels !== undefined) models = decodeSubscriptionPayload(encodedModels, missingModels)
    if (!String(leases ?? '').trim()) throw new Error(missingLeases)
    if (!String(models ?? '').trim()) throw new Error(missingModels)
    let parsedModels
    let compactLeases
    try { parsedModels = JSON.parse(models) } catch { throw new Error(missingModels) }
    try { compactLeases = JSON.stringify(JSON.parse(leases)) } catch { throw new Error(missingLeases) }
    try { subscriptionAccounts(parsedModels, JSON.parse(compactLeases)) } catch { throw new Error(missingLeases) }
    const leasesFile = path.join(root, 'leases.json')
    fs.writeFileSync(leasesFile, compactLeases, { mode: 0o600 })
    step = 'guest/nightly-assets'
    const release = await (fetchRelease ?? (() => loadRelease(repository, token)))()
    const { packageAsset, signatureAsset } = selectAssets(release, sourceSha, platform)
    const packageFile = path.join(root, packageAsset.name)
    const signatureFile = path.join(root, `${packageAsset.name}.sig`)
    if (fetchAsset) {
      fs.writeFileSync(packageFile, fetchAsset(packageAsset), { mode: 0o600 })
      fs.writeFileSync(signatureFile, fetchAsset(signatureAsset), { mode: 0o600 })
    } else {
      fs.writeFileSync(packageFile, await downloadAsset(repository, token, packageAsset.id), { mode: 0o600 })
      fs.writeFileSync(signatureFile, await downloadAsset(repository, token, signatureAsset.id), { mode: 0o600 })
    }
    const candidateFile = path.join(root, 'candidate.json')
    fs.writeFileSync(candidateFile, JSON.stringify({
      source_sha: sourceSha, platform, asset: packageAsset.name,
      sha256: createHash('sha256').update(fs.readFileSync(packageFile)).digest('hex'), models: parsedModels,
    }) + '\n', { mode: 0o600 })
    step = 'guest/updater-signature'
    const comment = verifyUpdaterSignature(fs.readFileSync(packageFile), fs.readFileSync(signatureFile, 'utf8'),
      fs.readFileSync('src-tauri/updater.pub', 'utf8'))
    const stableName = packageAsset.name.replace(`nightly-${sourceSha}-${platform.startsWith('macos-') ? 'macos' : platform}-`, '')
    if (!comment.split('\t').includes(`file:${stableName}`)) throw new Error(missingPackage)
    if (platform === 'windows') {
      step = 'guest/windows-job-test'
      if (runtime !== 'win32') throw new Error('Run this check on the requested native platform and architecture.')
      const result = spawnSync(process.execPath, ['--test', 'test/subscription-windows-process.node.mjs'], {
        timeout: 300_000, windowsHide: true, encoding: 'utf8',
      })
      if (result.error || result.status !== 0) throw nativeFailure('windows-job-test', result)
    }
    step = 'guest/install'
    const executable = install(platform, packageFile, root)
    if (platform === 'linux') {
      step = 'guest/linux-sandbox'
      if (runtime !== 'linux') throw new Error('Run this check on the requested native platform and architecture.')
      cleanupSandbox = prepareLinuxSandbox(packageFile, root)
    }
    process.env.MUNIMENT_NATIVE_DISPOSABLE_USER = '1'
    const probe = { candidateFile, packageFile, signatureFile, executable, leasesFile, output: artifacts, sourceSha, platform }
    if (platform.startsWith('macos-')) {
      step = 'guest/keychain-session'
      return runMacosProbe(probe)
    }
    return await run(probe)
  } catch (error) {
    const known = [missingPackage, missingLeases, missingModels,
      'Run this check on the requested native platform and architecture.']
    const reason = known.includes(error?.message) ? error.message
      : step === 'guest/keychain-session' ? 'The installed macOS probe or disposable Keychain session failed.'
        : step === 'guest/linux-sandbox' ? sandboxRequirement : missingPackage
    const redact = subscriptionRedactor({ leases, values: [token, encodedLeases] })
    writeBlocked(artifacts, sourceSha, platform, reason,
      `step=${step}\nerror=${error.message}\ninstall tail:\n${readDiagnosticLog(path.join(root, 'install.log'), root, redact)}`, redact)
    console.error(reason)
    return 1
  } finally {
    try { cleanupSandbox?.() } catch (error) {
      const redact = subscriptionRedactor({ leases, values: [token, encodedLeases] })
      writeBlocked(artifacts, sourceSha, platform, 'The Chromium sandbox helper cleanup failed.',
        `step=guest/linux-sandbox-cleanup\nerror=${error.message}`, redact)
      return 1
    } finally { fs.rmSync(root, { recursive: true, force: true }) }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = await guest({
    sourceSha: process.env.MUNIMENT_E2E_SOURCE_SHA,
    platform: process.env.MUNIMENT_SUBSCRIPTION_PLATFORM,
    leases: process.env.MUNIMENT_SUBSCRIPTION_LEASES,
    models: process.env.MUNIMENT_SUBSCRIPTION_MODELS,
    encodedLeases: process.env.MUNIMENT_SUBSCRIPTION_LEASES_BASE64,
    encodedModels: process.env.MUNIMENT_SUBSCRIPTION_MODELS_BASE64,
    repository: process.env.GITHUB_REPOSITORY,
    token: process.env.GH_TOKEN,
  })
}
