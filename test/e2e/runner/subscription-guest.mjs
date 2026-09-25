import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'
import { createHash } from 'node:crypto'
import { platforms } from '../support/subscription-acceptance.mjs'
import { run, writeBlocked } from './subscriptions.mjs'

const missingPackage = 'The signed nightly package or updater signature for this platform is missing.'
const missingLeases = 'Provide the FACTORY_SUBSCRIPTION_LEASES secret with access-only factory leases.'
const missingModels = 'Provide the FACTORY_SUBSCRIPTION_MODELS variable with four distinct supported model IDs.'

function packagePattern(sourceSha, platform) {
  if (platform === 'linux') return new RegExp(`^nightly-${sourceSha}-linux-muniment_[0-9]+\\.[0-9]+\\.[0-9]+_amd64\\.AppImage$`)
  if (platform === 'windows') return new RegExp(`^nightly-${sourceSha}-windows-muniment_[0-9]+\\.[0-9]+\\.[0-9]+_x64_en-US\\.msi$`)
  if (platform === 'macos-arm64') return new RegExp(`^nightly-${sourceSha}-macos-muniment-arm64\\.app\\.tar\\.gz$`)
  if (platform === 'macos-x64') return new RegExp(`^nightly-${sourceSha}-macos-muniment-x64\\.app\\.tar\\.gz$`)
  throw new Error('Run this check on the requested native platform and architecture.')
}

function downloadAsset(repository, token, assetId, destination) {
  const file = fs.openSync(destination, 'w', 0o600)
  try {
    const result = spawnSync('gh', ['api', '-H', 'Accept: application/octet-stream', `repos/${repository}/releases/assets/${assetId}`], {
      env: { ...process.env, GH_TOKEN: token }, stdio: ['ignore', file, 'pipe'], timeout: 180_000,
    })
    if (result.status !== 0) throw new Error(missingPackage)
  } finally {
    fs.closeSync(file)
  }
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
    if (result.status !== 0) throw new Error(missingPackage)
    const executable = path.join(expanded, 'muniment.app', 'Contents', 'MacOS', 'muniment-desktop')
    if (!fs.existsSync(executable)) throw new Error(missingPackage)
    return executable
  }
  const result = spawnSync('msiexec.exe', ['/i', packageFile, '/qn', '/norestart'], { timeout: 180_000, windowsHide: true })
  if (result.status !== 0 && result.status !== 3010) throw new Error(missingPackage)
  const executable = path.join(process.env.LOCALAPPDATA, 'muniment', 'muniment-desktop.exe')
  if (!fs.existsSync(executable)) throw new Error(missingPackage)
  return executable
}

export async function guest({ sourceSha, platform, output, leases, models, repository, token, fetchRelease, fetchAsset }) {
  if (!platforms.includes(platform) || !/^[a-f0-9]{40}$/.test(sourceSha ?? '') || !output) {
    throw new Error('Provide a supported native platform, output directory, and the exact candidate source SHA.')
  }
  writeBlocked(output, sourceSha, platform, 'Provide the pinned signed package, a native GUI runner, and fresh factory subscription access leases.')
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-guest-'))
  try {
    if (!String(leases ?? '').trim()) throw new Error(missingLeases)
    if (!String(models ?? '').trim()) throw new Error(missingModels)
    let parsedModels
    let compactLeases
    try { parsedModels = JSON.parse(models) } catch { throw new Error(missingModels) }
    try { compactLeases = JSON.stringify(JSON.parse(leases)) } catch { throw new Error(missingLeases) }
    const leasesFile = path.join(root, 'leases.json')
    fs.writeFileSync(leasesFile, compactLeases, { mode: 0o600 })
    const loadRelease = fetchRelease ?? (() => {
      const result = spawnSync('gh', ['api', `repos/${repository}/releases/tags/nightly`], {
        env: { ...process.env, GH_TOKEN: token }, encoding: 'utf8', timeout: 60_000,
      })
      if (result.status !== 0) throw new Error(missingPackage)
      return JSON.parse(result.stdout)
    })
    const { packageAsset, signatureAsset } = selectAssets(loadRelease(), sourceSha, platform)
    const packageFile = path.join(root, packageAsset.name)
    const signatureFile = path.join(root, `${packageAsset.name}.sig`)
    if (fetchAsset) {
      fs.writeFileSync(packageFile, fetchAsset(packageAsset), { mode: 0o600 })
      fs.writeFileSync(signatureFile, fetchAsset(signatureAsset), { mode: 0o600 })
    } else {
      downloadAsset(repository, token, packageAsset.id, packageFile)
      downloadAsset(repository, token, signatureAsset.id, signatureFile)
    }
    const candidateFile = path.join(root, 'candidate.json')
    fs.writeFileSync(candidateFile, JSON.stringify({
      source_sha: sourceSha, platform, asset: packageAsset.name,
      sha256: createHash('sha256').update(fs.readFileSync(packageFile)).digest('hex'), models: parsedModels,
    }) + '\n', { mode: 0o600 })
    const executable = install(platform, packageFile, root)
    process.env.MUNIMENT_NATIVE_DISPOSABLE_USER = '1'
    return await run({ candidateFile, packageFile, signatureFile, executable, leasesFile, output, sourceSha, platform })
  } catch (error) {
    const known = [missingPackage, missingLeases, missingModels,
      'Run this check on the requested native platform and architecture.']
    const reason = known.includes(error?.message) ? error.message : missingPackage
    writeBlocked(output, sourceSha, platform, reason)
    console.error(reason)
    return 1
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = await guest({
    sourceSha: process.env.MUNIMENT_E2E_SOURCE_SHA,
    platform: process.env.MUNIMENT_SUBSCRIPTION_PLATFORM,
    output: process.env.DCI_ARTIFACTS_DIR || process.env.OUTPUT,
    leases: process.env.MUNIMENT_SUBSCRIPTION_LEASES,
    models: process.env.MUNIMENT_SUBSCRIPTION_MODELS,
    repository: process.env.GITHUB_REPOSITORY,
    token: process.env.GH_TOKEN,
  })
}
