import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { randomBytes } from 'node:crypto'
import { spawn, spawnSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'
import { acceptance, blocked, checkIdentity, hash, platforms, subscriptionAccounts } from '../support/subscription-acceptance.mjs'
import { verifyUpdaterSignature } from '../../../.github/lib/updater-signature.mjs'

const delay = ms => new Promise(resolve => setTimeout(resolve, ms))
const nativePlatform = () => process.platform === 'darwin' ? `macos-${process.arch === 'arm64' ? 'arm64' : 'x64'}`
  : process.platform === 'win32' ? 'windows' : process.platform
const json = file => JSON.parse(fs.readFileSync(file, 'utf8'))
const save = (file, value) => fs.writeFileSync(file, JSON.stringify(value, null, 2) + '\n', { mode: 0o600 })
function execute(command, args, env, timeout = 180_000) {
  const result = spawnSync(command, args, { env, timeout, stdio: 'pipe', windowsHide: true })
  if (result.error || result.status !== 0) throw new Error('A native prerequisite or package check failed. Check the runner setup.')
  return result.stdout.toString('utf8').trim()
}

// Do not inherit factory tokens, provider homes, Pi settings, proxies, or gh credentials.
export function isolatedEnvironment(root, source = process.env) {
  const env = {}
  for (const name of ['PATH', 'SystemRoot', 'WINDIR', 'COMSPEC', 'PATHEXT', 'DISPLAY', 'WAYLAND_DISPLAY', 'DBUS_SESSION_BUS_ADDRESS', 'XAUTHORITY', 'LANG']) {
    if (source[name]) env[name] = source[name]
  }
  for (const [name, directory] of Object.entries({ HOME: 'home', USERPROFILE: 'home', APPDATA: 'roaming', LOCALAPPDATA: 'local',
    XDG_CONFIG_HOME: 'config', XDG_DATA_HOME: 'data', XDG_CACHE_HOME: 'cache', XDG_RUNTIME_DIR: 'runtime',
    TMPDIR: 'tmp', TEMP: 'tmp', TMP: 'tmp', MUNIMENT_STATE_DIR: 'state' })) {
    env[name] = path.join(root, directory)
    fs.mkdirSync(env[name], { recursive: true, mode: 0o700 })
  }
  env.PI_CODING_AGENT_DIR = path.join(env.MUNIMENT_STATE_DIR, 'agent')
  fs.mkdirSync(env.PI_CODING_AGENT_DIR, { mode: 0o700 })
  env.MUNIMENT_SUBSCRIPTION_PROBE = '1'
  return env
}

export function tree(root) {
  const entries = {}
  function visit(directory, prefix = '') {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const relative = prefix + entry.name
      const file = path.join(directory, entry.name)
      if (entry.isSymbolicLink()) entries[relative] = `link:${fs.readlinkSync(file)}`
      else if (entry.isDirectory()) visit(file, relative + '/')
      else if (entry.isFile()) entries[relative] = hash(fs.readFileSync(file))
      else throw new Error('The installed payload contains an unsupported file.')
    }
  }
  visit(root)
  return entries
}
function equalPayload(expected, installed) {
  const keys = Object.keys(expected)
  if (!keys.length || keys.length !== Object.keys(installed).length || keys.some(key => installed[key] !== expected[key])) {
    throw new Error('The installed payload does not match the signed package.')
  }
}
function locate(root, name) {
  const matches = Object.keys(tree(root)).filter(file => path.basename(file) === name)
  if (matches.length !== 1) throw new Error('The package does not contain one desktop executable.')
  return path.join(root, matches[0])
}

function verifyInstalled(candidate, packageFile, executable, root, env) {
  const expanded = path.join(root, 'expanded')
  fs.mkdirSync(expanded)
  if (candidate.platform === 'linux') {
    if (!candidate.asset.endsWith('_amd64.AppImage') || hash(fs.readFileSync(executable)) !== candidate.sha256) {
      throw new Error('Install the pinned signed AppImage without changing its bytes.')
    }
    return () => {
      if (hash(fs.readFileSync(executable)) !== candidate.sha256) throw new Error('The installed AppImage changed during the probe.')
    }
  }
  let payload, installed
  if (candidate.platform.startsWith('macos-')) {
    const suffix = candidate.platform === 'macos-arm64' ? '-arm64.app.tar.gz' : '-x64.app.tar.gz'
    if (!candidate.asset.endsWith(suffix)) throw new Error('Use the architecture-specific signed macOS update package.')
    const names = execute('tar', ['-tzf', packageFile], env).split('\n')
    if (names.some(name => path.posix.isAbsolute(name) || name.split('/').includes('..'))) throw new Error('The package paths are invalid.')
    execute('tar', ['-xzf', packageFile, '-C', expanded], env)
    payload = path.join(expanded, 'muniment.app')
    installed = path.resolve(executable, '../../..')
    if (path.relative(installed, executable) !== path.join('Contents', 'MacOS', 'muniment-desktop')) {
      throw new Error('Select the installed desktop inside the signed app bundle.')
    }
    execute('codesign', ['--verify', '--deep', '--strict', installed], env)
    execute('spctl', ['--assess', '--type', 'execute', installed], env)
  } else {
    if (!candidate.asset.endsWith('_x64_en-US.msi')) throw new Error('Use the signed per-user Windows MSI.')
    execute('msiexec.exe', ['/a', packageFile, '/qn', `TARGETDIR=${expanded}`], env)
    payload = path.dirname(locate(expanded, 'muniment-desktop.exe'))
    installed = path.dirname(executable)
    execute('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
      'if ((Get-AuthenticodeSignature -LiteralPath $env:MUNIMENT_SUBSCRIPTION_PACKAGE).Status -ne "Valid") { exit 1 }'],
    { ...env, MUNIMENT_SUBSCRIPTION_PACKAGE: packageFile })
  }
  const expected = tree(payload)
  const before = tree(installed)
  equalPayload(expected, before)
  return () => {
    const after = tree(installed)
    equalPayload(expected, after)
    if (JSON.stringify(before) !== JSON.stringify(after)) throw new Error('The installed payload changed during the probe.')
  }
}

function screenshot(platform, pid, output, env) {
  if (platform === 'linux') {
    const ids = execute('xdotool', ['search', '--onlyvisible', '--pid', String(pid), '--name', '^muniment$'], env, 10_000).split(/\s+/)
    if (ids.length !== 1 || !/^\d+$/.test(ids[0])) throw new Error('The native screenshot requires one installed app window.')
    execute('import', ['-window', ids[0], output], env, 10_000)
  } else if (platform.startsWith('macos-')) {
    // Screen capture needs a dedicated GUI login with Screen Recording permission.
    const helper = path.join(env.TMPDIR, 'window-id')
    execute('clang', ['-std=gnu17', '-framework', 'CoreFoundation', '-framework', 'CoreGraphics',
      '-o', helper, 'test/e2e/support/macos-window-count.c'], env)
    const id = execute(helper, [String(pid), '--id'], env, 10_000)
    if (!/^\d+$/.test(id)) throw new Error('The native screenshot requires the installed app window.')
    execute('screencapture', ['-x', `-l${id}`, output], env, 10_000)
  } else {
    execute('powershell.exe', ['-NoProfile', '-NonInteractive', '-File', path.resolve('test/e2e/support/subscription-screenshot.ps1'),
      '-AppPid', String(pid), '-Destination', output], env, 10_000)
  }
}

export const updaterPublicKeyFile = 'src-tauri/updater.pub'

export function writeBlocked(output, sourceSha, platform, reason) {
  fs.mkdirSync(output, { recursive: true, mode: 0o700 })
  save(path.join(output, 'release-acceptance.json'), blocked(sourceSha, platform, reason))
  save(path.join(output, `${platform}-subscription.json`), { status: 'blocked', reason })
  fs.rmSync(path.join(output, `screenshot-${platform}-subscriptions.png`), { force: true })
}

export async function run({ candidateFile, packageFile, signatureFile, executable, leasesFile, output, sourceSha, platform, publicKeyFile = updaterPublicKeyFile }) {
  if (!platforms.includes(platform) || !/^[a-f0-9]{40}$/.test(sourceSha ?? '')) {
    throw new Error('Provide a supported native platform and the exact candidate source SHA.')
  }
  const proofFile = path.join(output, 'release-acceptance.json')
  const evidenceFile = path.join(output, `${platform}-subscription.json`)
  // Write a blocked result first so a killed runner cannot leave stale passing evidence.
  let reason = 'Provide the pinned signed package, a native GUI runner, and fresh factory subscription access leases.'
  writeBlocked(output, sourceSha, platform, reason)
  let root, env, app, appClosed, runtime, runtimeClosed, verify, candidate, packageName
  let passed = false
  let evidence, proof
  try {
    reason = 'Run this check on the requested native platform and architecture.'
    if (!platforms.includes(platform) || platform !== nativePlatform() ||
        !['x64', 'arm64'].includes(process.arch) || (platform === 'linux' && process.arch !== 'x64')) {
      throw new Error('Run this check on the requested native platform and architecture.')
    }
    reason = 'Use a disposable native GUI login. Set MUNIMENT_NATIVE_DISPOSABLE_USER=1 only for that login.'
    if (process.env.MUNIMENT_NATIVE_DISPOSABLE_USER !== '1') {
      throw new Error('Use a disposable native GUI login. Set MUNIMENT_NATIVE_DISPOSABLE_USER=1 only for that login.')
    }
    reason = 'Provide the candidate manifest, signed package, signature, installed app, and factory subscription lease file.'
    if (![candidateFile, packageFile, signatureFile, executable, leasesFile].every(file => file && fs.existsSync(file))) throw new Error(reason)
    reason = 'The candidate identity, package digest, or updater signature is invalid.'
    candidate = json(candidateFile)
    const bytes = fs.readFileSync(packageFile)
    packageName = checkIdentity(candidate, sourceSha, platform, bytes)
    const publicKey = fs.readFileSync(publicKeyFile || updaterPublicKeyFile, 'utf8')
    const comment = verifyUpdaterSignature(bytes, fs.readFileSync(signatureFile, 'utf8'), publicKey)
    if (!comment.split('\t').includes(`file:${packageName}`)) throw new Error('The signed package name does not match the candidate.')
    reason = 'Provide four distinct models and access-only factory subscription leases valid for at least 20 minutes.'
    const leaseStat = fs.statSync(leasesFile)
    if (!leaseStat.isFile() || (process.platform !== 'win32' && (leaseStat.mode & 0o077) !== 0)) {
      throw new Error('Keep the factory lease file readable only by its owner.')
    }
    const accounts = subscriptionAccounts(candidate.models, json(leasesFile))
    root = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-subscriptions-'))
    env = isolatedEnvironment(root)
    reason = 'The installed payload does not match the signed package. Check native package and signature tools.'
    const verifyPayload = verifyInstalled(candidate, packageFile, executable, root, env)
    verify = () => {
      verifyPayload()
      if (hash(fs.readFileSync(packageFile)) !== candidate.sha256) throw new Error('The candidate package changed during the probe.')
    }
    const nonce = `MUNIMENT-${randomBytes(16).toString('hex')}`
    const state = env.MUNIMENT_STATE_DIR
    save(path.join(state, 'subscription-probe.json'), { models: candidate.models, nonce })
    fs.writeFileSync(path.join(state, '.adopted'), '', { mode: 0o600 })
    fs.writeFileSync(path.join(state, 'local-mode'), '1', { mode: 0o600 })
    save(path.join(env.PI_CODING_AGENT_DIR, 'muniment-router.json'), { enabled: true, accounts })
    save(path.join(env.PI_CODING_AGENT_DIR, 'auth.json'), {})
    save(path.join(env.PI_CODING_AGENT_DIR, 'settings.json'), {
      defaultProvider: 'muniment-router', defaultModel: `${candidate.models[0].family}/${candidate.models[0].id}`,
    })
    reason = 'The installed probe did not finish. Check the native runtime, subscription access, and selected models.'
    // Start the installed runtime with the same disposable profile on service-based platforms.
    if (platform !== 'linux') {
      const runtimeFile = platform === 'windows' ? path.join(path.dirname(executable), 'muniment-runtime.exe')
        : path.resolve(executable, '../../Library/LaunchServices/muniment-runtime')
      runtime = spawn(runtimeFile, [], { env, stdio: 'ignore', detached: process.platform !== 'win32' })
      runtimeClosed = new Promise(resolve => { runtime.once('exit', resolve); runtime.once('error', resolve) })
      await delay(3000)
      if (!runtime.pid || runtime.exitCode !== null) throw new Error('The installed runtime could not start with the disposable profile.')
    }
    app = spawn(executable, ['--probe-subscription-chat'], { env, stdio: 'ignore', detached: process.platform !== 'win32' })
    let appError = false
    appClosed = new Promise(resolve => {
      app.once('exit', resolve)
      app.once('error', () => { appError = true; resolve() })
    })
    const resultFile = path.join(state, 'subscription-probe-result.json')
    const deadline = Date.now() + 15 * 60_000
    while (!fs.existsSync(resultFile) && Date.now() < deadline && app.exitCode === null && !appError) await delay(250)
    if (!fs.existsSync(resultFile)) throw new Error('The installed probe did not finish. Check the native runtime and subscription access.')
    const probe = json(resultFile)
    reason = 'The installed subscription reply or model switch failed. Check model access and thread continuity.'
    if (!probe.passed) throw new Error('The installed subscription reply or model switch failed.')
    reason = 'The native model receipts or package identity failed verification. Check requested models, compiled source, and signed package.'
    const transports = fs.readFileSync(path.join(env.PI_CODING_AGENT_DIR, 'subscription-probe-transports.jsonl'), 'utf8').trim().split('\n').map(JSON.parse)
    verify()
    const result = { status: 'passed', installed: true, unchanged: true, source_sha: probe.source_sha, webdriver: probe.webdriver,
      package_sha256: candidate.sha256, turns: probe.turns.map(turn => ({ ...turn,
        requested: candidate.models[turn.index]?.id, expected: nonce })) }
    proof = acceptance(candidate, sourceSha, platform, result, transports, packageName)
    reason = 'The native screenshot failed. Install the capture tool and grant the disposable login screen capture access.'
    const raw = path.join(root, 'capture')
    const safe = path.join(root, 'safe')
    fs.mkdirSync(raw)
    const screenshotName = `screenshot-${platform}-subscriptions.png`
    screenshot(platform, app.pid, path.join(raw, screenshotName), env)
    // Keep the redactor's injected-secret screening separate from the app environment.
    execute(process.execPath, ['test/e2e/support/redact.mjs', raw, safe], process.env)
    fs.copyFileSync(path.join(safe, screenshotName), path.join(output, screenshotName))
    evidence = { ...result, transports }
    passed = true
  } catch {
    // Never export parser excerpts, native stderr, provider errors, or conversation logs.
    writeBlocked(output, sourceSha, platform, reason)
  } finally {
    let cleanupFailed = false
    for (const [child, closed] of [[app, appClosed], [runtime, runtimeClosed]]) {
      try {
        if (!child?.pid) continue
        if (process.platform === 'win32') {
          if (child.exitCode === null) execute('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], env, 15_000)
        } else {
          try { process.kill(-child.pid, 'SIGKILL') } catch (error) { if (error.code !== 'ESRCH') throw error }
        }
        await Promise.race([closed, delay(5000).then(() => { throw new Error('The native probe process did not stop.') })])
      } catch { cleanupFailed = true }
    }
    try {
      verify?.()
    } catch { cleanupFailed = true }
    try {
      if (root) fs.rmSync(root, { recursive: true, force: true })
    } catch { cleanupFailed = true }
    if (cleanupFailed) {
      passed = false
      reason = 'The native probe cleanup failed. Stop the disposable login before another check.'
      writeBlocked(output, sourceSha, platform, reason)
    }
  }
  if (passed) {
    save(evidenceFile, evidence)
    save(proofFile, proof)
  }
  return passed ? 0 : 1
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [candidateFile, packageFile, signatureFile, executable, leasesFile, output, sourceSha, platform] = process.argv.slice(2)
  if (!output || !/^[a-f0-9]{40}$/.test(sourceSha ?? '') || !platforms.includes(platform)) {
    console.error('Use subscriptions.mjs CANDIDATE PACKAGE SIGNATURE INSTALLED_APP LEASES OUTPUT SOURCE_SHA PLATFORM.')
    process.exitCode = 1
  } else process.exitCode = await run({ candidateFile, packageFile, signatureFile, executable, leasesFile, output, sourceSha, platform })
}
