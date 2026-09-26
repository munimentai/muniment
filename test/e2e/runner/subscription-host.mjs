import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'
import { platforms } from '../support/subscription-acceptance.mjs'
import { writeBlocked } from './subscriptions.mjs'

const desktopCiName = platform => platform === 'macos-x64' ? 'macos' : platform

function compactJson(value, reason) {
  try {
    const parsed = JSON.parse(value)
    if (parsed === null) throw new Error(reason)
    return JSON.stringify(parsed)
  } catch {
    throw new Error(reason)
  }
}

export function runDesktopCi({ sourceSha, platform, subscriptionPlatform, output, leases, models, repository, token, sshKey, knownHosts, harnessSha, spawnProcess = spawnSync }) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-host-'))
  try {
    const keyFile = path.join(root, 'key')
    const hostsFile = path.join(root, 'known_hosts')
    const transcript = path.join(root, 'output')
    fs.writeFileSync(keyFile, sshKey, { mode: 0o600 })
    fs.writeFileSync(hostsFile, knownHosts.endsWith('\n') ? knownHosts : `${knownHosts}\n`, { mode: 0o600 })
    let cmd = 'node test/e2e/runner/subscription-guest.mjs'
    if (platform === 'linux') {
      cmd = 'sudo apt-get update -qq && sudo apt-get install -y -qq --no-install-recommends xvfb xdotool imagemagick dbus-x11 fuse3 && dbus-run-session -- xvfb-run -a node test/e2e/runner/subscription-guest.mjs'
    }
    const extra = platform === 'windows' ? ' --console-user' : platform === 'macos' ? ' --screendump' : ''
    const ref = /^[a-f0-9]{40}$/.test(harnessSha ?? '') ? harnessSha : sourceSha
    const remote = `sudo desktop-ci ${platform}${extra} --repo 'https://github.com/${repository}.git' --ref '${ref}' --cmd '${cmd}' --env-stdin --memory 8192 --build-timeout 2400 --collect-artifacts`
    const input = [
      'MUNIMENT_PI_CANDIDATE=1',
      `GH_TOKEN=${token}`,
      `GITHUB_REPOSITORY=${repository}`,
      `MUNIMENT_E2E_SOURCE_SHA=${sourceSha}`,
      `MUNIMENT_SUBSCRIPTION_PLATFORM=${subscriptionPlatform}`,
      // POSIX guests source these lines. Windows guests read them as literal values.
      `MUNIMENT_SUBSCRIPTION_LEASES_BASE64=${Buffer.from(leases, 'utf8').toString('base64')}`,
      `MUNIMENT_SUBSCRIPTION_MODELS_BASE64=${Buffer.from(models, 'utf8').toString('base64')}`,
      'MUNIMENT_NATIVE_DISPOSABLE_USER=1',
    ].join('\n') + '\n'
    const ssh = spawnProcess('ssh', [
      '-i', keyFile, '-o', 'StrictHostKeyChecking=yes', '-o', `UserKnownHostsFile=${hostsFile}`,
      '-o', 'BatchMode=yes', '-o', 'ServerAliveInterval=30', '-o', 'ServerAliveCountMax=8',
      'desktopci@10.1.10.10', remote,
    ], { input, encoding: 'utf8', timeout: 40 * 60_000 })
    fs.writeFileSync(transcript, `${ssh.stdout ?? ''}${ssh.stderr ?? ''}`)
    const extract = spawnProcess('bash', ['test/e2e/support/extract-artifacts.sh', transcript, output, String(ssh.status ?? 1)])
    return { status: ssh.status === 0 && extract.status === 0 ? 0 : 1 }
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
}

export function host({ sourceSha, platform, output, leases, models, repository, token, sshKey, knownHosts, harnessSha, invoke = runDesktopCi }) {
  if (!platforms.includes(platform) || !/^[a-f0-9]{40}$/.test(sourceSha ?? '')) {
    throw new Error('Provide a supported native platform and the exact candidate source SHA.')
  }
  const missingRunner = platform === 'macos-arm64'
    ? 'Use the macos-15 GitHub-hosted ARM64 job. The factory macOS runner supports Intel only.'
    : 'The native desktop-ci runner for this platform is unavailable.'
  const missingLeases = 'Provide the FACTORY_SUBSCRIPTION_LEASES secret with access-only factory leases.'
  const missingModels = 'Provide the FACTORY_SUBSCRIPTION_MODELS variable with four distinct supported model IDs.'
  let reason = 'Provide the pinned signed package, a native GUI runner, and fresh factory subscription access leases.'
  writeBlocked(output, sourceSha, platform, reason)
  try {
    if (!String(leases ?? '').trim()) throw new Error(missingLeases)
    if (!String(models ?? '').trim()) throw new Error(missingModels)
    const compactLeases = compactJson(leases, missingLeases)
    const compactModels = compactJson(models, missingModels)
    if (platform === 'macos-arm64' || !sshKey || !knownHosts) throw new Error(missingRunner)
    const artifacts = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-guest-artifacts-'))
    try {
      const result = invoke({
        sourceSha, platform: desktopCiName(platform), subscriptionPlatform: platform, output: artifacts,
        leases: compactLeases, models: compactModels, repository, token, sshKey, knownHosts, harnessSha,
      })
      const evidence = path.join(artifacts, `${platform}-subscription.json`)
      const proof = path.join(artifacts, 'release-acceptance.json')
      if (!fs.existsSync(evidence) || !fs.existsSync(proof)) throw new Error(missingRunner)
      for (const name of fs.readdirSync(artifacts)) {
        const file = path.join(artifacts, name)
        if (!fs.statSync(file).isFile()) continue
        fs.copyFileSync(file, path.join(output, name))
      }
      return result.status === 0 ? 0 : 1
    } finally {
      fs.rmSync(artifacts, { recursive: true, force: true })
    }
  } catch (error) {
    reason = error instanceof Error ? error.message : missingRunner
    writeBlocked(output, sourceSha, platform, reason)
    console.error(reason)
    return 1
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = host({
    sourceSha: process.env.SOURCE_SHA,
    platform: process.env.PLATFORM,
    output: process.env.OUTPUT,
    leases: process.env.FACTORY_SUBSCRIPTION_LEASES,
    models: process.env.FACTORY_SUBSCRIPTION_MODELS,
    repository: process.env.REPOSITORY,
    token: process.env.RELEASE_TOKEN,
    sshKey: process.env.DESKTOP_CI_SSH_KEY,
    knownHosts: process.env.DESKTOP_CI_KNOWN_HOSTS,
    harnessSha: process.env.GITHUB_SHA,
  })
}
