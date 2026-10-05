import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'
import { platforms, subscriptionAccounts } from '../support/subscription-acceptance.mjs'
import { writeBlocked } from './subscriptions.mjs'
import { copyWindowsUpdate } from '../support/subscription-windows-update.mjs'
import { subscriptionRedactor, diagnosticTail, nativeFailure, transcriptText, reportPayloadDifferences, reportSubscriptionFailure } from '../support/subscription-diagnostics.mjs'

const desktopCiName = platform => platform === 'macos-x64' ? 'macos' : platform

// Allow setup, artifact collection, and cleanup outside the guest build budget.
export const desktopCiBudget = Object.freeze({ slot: 7200, build: 2400, run: 3600, cleanup: 30, client: 11100 })
const desktopCiReasons = Object.freeze({
  'slot-wait': 'The desktop-CI slot stayed busy for the 120-minute wait limit.',
  guest: 'The desktop-CI guest exceeded its 40-minute build timeout.',
  run: 'The desktop-CI guest and artifact collection exceeded their 60-minute limit.',
  startup: 'The desktop-CI driver did not report a slot result within the 180-minute limit.',
  client: 'The desktop-CI SSH session exceeded its 185-minute limit.',
  'sudo-denied': 'The desktop-CI host denied permission to start the driver.',
})
const shellQuote = value => `'${value.replaceAll("'", "'\\''")}'`

function desktopCiFailure(ssh) {
  if (ssh.error?.code === 'ETIMEDOUT') return 'client'
  if (ssh.status === 0 || ssh.status === 255 || ssh.status === null) return undefined
  const transcript = `${ssh.stdout ?? ''}\n${ssh.stderr ?? ''}`
  const timeout = transcript.match(/^\[subscription-host\] timeout=(slot-wait|run|startup)$/m)
  if (ssh.status === 124 && timeout) return timeout[1]
  if (!/^\[desktop-ci \d{2}:\d{2}:\d{2}\]/m.test(transcript) &&
      /^(?:sudo:.*(?:password is required|not allowed|not in the sudoers|[Pp]ermission denied)|Sorry, user .* is not allowed to execute|.* is not in the sudoers file|(?:sh|bash):.*[Pp]ermission denied)/m.test(transcript)) return 'sudo-denied'
  if (/^\[desktop-ci \d{2}:\d{2}:\d{2}\] BUILD FAILED \((linux|windows|macos)\) rc=124\r?$/m.test(transcript)) return 'guest'
  if (ssh.status === 124 && !transcript.includes('BUILD GREEN') &&
      /^\[desktop-ci \d{2}:\d{2}:\d{2}\] SSH up; starting repo build \(timeout 2400s\)\r?$/m.test(transcript)) return 'guest'
}

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
  const redact = subscriptionRedactor({ leases, values: [token, sshKey] })
  let detail = 'step=desktop-ci/ssh\n'
  try {
    const keyFile = path.join(root, 'key')
    const hostsFile = path.join(root, 'known_hosts')
    const transcript = path.join(root, 'output')
    fs.writeFileSync(keyFile, sshKey, { mode: 0o600 })
    fs.writeFileSync(hostsFile, knownHosts.endsWith('\n') ? knownHosts : `${knownHosts}\n`, { mode: 0o600 })
    let cmd = 'node test/e2e/runner/subscription-guest.mjs'
    if (platform === 'linux') {
      cmd = 'sudo apt-get update -qq && sudo apt-get install -y -qq --no-install-recommends xvfb xdotool imagemagick dbus-x11 fuse3 openssl && dbus-run-session -- xvfb-run -a node test/e2e/runner/subscription-guest.mjs'
    }
    const extra = platform === 'windows' ? ' --console-user' : platform === 'macos' ? ' --screendump' : ''
    const ref = /^[a-f0-9]{40}$/.test(harnessSha ?? '') ? harnessSha : sourceSha
    const wrapper = fs.readFileSync(new URL('../support/desktop-ci-budget.py', import.meta.url), 'utf8')
    const remote = `python3 -c ${shellQuote(wrapper)} ${desktopCiBudget.slot} ${desktopCiBudget.run} ${desktopCiBudget.cleanup} sudo -n desktop-ci ${platform}${extra} --repo 'https://github.com/${repository}.git' --ref '${ref}' --cmd '${cmd}' --env-stdin --memory 8192 --build-timeout ${desktopCiBudget.build} --collect-artifacts`
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
      '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=30', '-o', 'ServerAliveInterval=30', '-o', 'ServerAliveCountMax=8',
      'desktopci@10.1.10.10', remote,
    ], { input, encoding: 'utf8', timeout: desktopCiBudget.client * 1000, maxBuffer: 32 * 1024 * 1024 })
    fs.writeFileSync(transcript, `${ssh.stdout ?? ''}${ssh.stderr ?? ''}`, { mode: 0o600 })
    detail += `status=${ssh.status ?? 'none'} signal=${ssh.signal ?? 'none'} error=${redact(ssh.error?.message ?? 'none')}\n`
    detail += diagnosticTail(transcriptText(redact(nativeFailure('ssh', ssh).message)), redact)
    const extract = spawnProcess('bash', ['test/e2e/support/extract-artifacts.sh', transcript, output, String(ssh.status ?? 1)],
      { encoding: 'utf8', timeout: 60_000 })
    detail += `\nstep=desktop-ci/extract\n${diagnosticTail(nativeFailure('extract-artifacts', extract).message, redact)}`
    const failure = desktopCiFailure(ssh)
    return { status: !ssh.error && ssh.status === 0 && !extract.error && extract.status === 0 ? 0 : 1, ...(failure ? { failure } : {}) }
  } catch (error) {
    detail += `\nerror=${redact(error.message)}\n`
    throw error
  } finally {
    fs.mkdirSync(output, { recursive: true, mode: 0o700 })
    const log = path.join(output, `${subscriptionPlatform}-subscription.log`)
    const previous = fs.existsSync(log) ? fs.readFileSync(log, 'utf8') : ''
    fs.writeFileSync(log, redact(`${previous}\n${detail}\n`), { mode: 0o600 })
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
  const redact = subscriptionRedactor({ leases, values: [token, sshKey] })
  let status = 1
  fs.rmSync(path.join(output, `${platform}-subscription.log`), { force: true })
  if (platform === 'windows') {
    for (const name of ['windows-subscription-msi.log', 'windows-subscription-relaunch.json']) fs.rmSync(path.join(output, name), { force: true })
  }
  writeBlocked(output, sourceSha, platform, reason)
  try {
    if (!String(leases ?? '').trim()) throw new Error(missingLeases)
    if (!String(models ?? '').trim()) throw new Error(missingModels)
    const compactLeases = compactJson(leases, missingLeases)
    const compactModels = compactJson(models, missingModels)
    try { subscriptionAccounts(JSON.parse(compactModels), JSON.parse(compactLeases)) }
    catch { throw new Error(missingLeases) }
    if (platform === 'macos-arm64' || !sshKey || !knownHosts) throw new Error(missingRunner)
    const artifacts = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-guest-artifacts-'))
    try {
      let result
      try {
        result = invoke({
          sourceSha, platform: desktopCiName(platform), subscriptionPlatform: platform, output: artifacts,
          leases: compactLeases, models: compactModels, repository, token, sshKey, knownHosts, harnessSha,
        })
      } finally {
        if (platform === 'windows') copyWindowsUpdate(artifacts, output)
        const log = path.join(artifacts, `${platform}-subscription.log`)
        if (fs.existsSync(log) && fs.lstatSync(log).isFile()) {
          fs.writeFileSync(path.join(output, `${platform}-subscription.log`), redact(fs.readFileSync(log, 'utf8')), { mode: 0o600 })
        }
      }
      const evidence = path.join(artifacts, `${platform}-subscription.json`)
      const proof = path.join(artifacts, 'release-acceptance.json')
      const runnerReason = Object.hasOwn(desktopCiReasons, result.failure) ? desktopCiReasons[result.failure] : missingRunner
      if (!fs.existsSync(evidence) || !fs.existsSync(proof)) throw new Error(runnerReason)
      for (const name of ['release-acceptance.json', `${platform}-subscription.json`,
        `screenshot-${platform}-subscriptions.png`]) {
        const file = path.join(artifacts, name)
        if (!fs.existsSync(file) || !fs.lstatSync(file).isFile()) continue
        fs.copyFileSync(file, path.join(output, name))
      }
      if (result.status !== 0 && JSON.parse(fs.readFileSync(proof, 'utf8')).cases?.every(item => item.status === 'passed')) {
        writeBlocked(output, sourceSha, platform, runnerReason)
      }
      status = result.status === 0 ? 0 : 1
      return status
    } finally {
      fs.rmSync(artifacts, { recursive: true, force: true })
    }
  } catch (error) {
    reason = [missingLeases, missingModels, missingRunner, ...Object.values(desktopCiReasons)].includes(error?.message) ? error.message : missingRunner
    writeBlocked(output, sourceSha, platform, reason, `step=host\nerror=${error.message}`, redact)
    return 1
  } finally {
    reportSubscriptionFailure(output, platform, status, redact)
    reportPayloadDifferences(fs.readFileSync(path.join(output, `${platform}-subscription.log`), 'utf8'), redact)
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
    harnessSha: process.env.HARNESS_SHA ?? process.env.SOURCE_SHA,
  })
}
