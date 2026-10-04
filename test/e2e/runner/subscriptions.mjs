import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { randomBytes } from 'node:crypto'
import { spawn, spawnSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'
import { AcceptanceError, acceptance, blocked, chatTransports, checkIdentity, featureChecks, featureFailure, hash, platforms, subscriptionAccounts } from '../support/subscription-acceptance.mjs'
import { verifyUpdaterSignature } from '../../../.github/lib/updater-signature.mjs'
import { updateFixture } from './subscription-update.mjs'
import { subscriptionRedactor, nativeFailure, processStatus, profileLogs, linuxRuntimeStatus, probeProgress, probeFailure, payloadDifferenceDetail, reportPayloadDifferences } from '../support/subscription-diagnostics.mjs'
import { launchWindowsTree, stopWindowsTree, windowsTreeAlive } from '../support/subscription-windows-process.mjs'

const delay = ms => new Promise(resolve => setTimeout(resolve, ms))
const nativePlatform = () => process.platform === 'darwin' ? `macos-${process.arch === 'arm64' ? 'arm64' : 'x64'}`
  : process.platform === 'win32' ? 'windows' : process.platform
const json = file => JSON.parse(fs.readFileSync(file, 'utf8'))
const save = (file, value) => fs.writeFileSync(file, JSON.stringify(value, null, 2) + '\n', { mode: 0o600 })
function execute(command, args, env, timeout = 180_000) {
  const result = spawnSync(command, args, { env, timeout, stdio: 'pipe', windowsHide: true })
  if (result.error || result.status !== 0) {
    throw Object.assign(nativeFailure(command, result), { cause: result.error, signal: result.signal })
  }
  return result.stdout.toString('utf8').trim()
}

// Do not inherit factory tokens, provider homes, Pi settings, proxies, or gh credentials.
export function isolatedEnvironment(root, source = process.env, platform = nativePlatform()) {
  const env = {}
  // Native services use the disposable login, not a synthetic OS profile.
  // Windows expands known folders through USERPROFILE. macOS reads Keychain preferences through HOME.
  const nativeHomes = platform === 'windows' ? ['USERPROFILE', 'APPDATA', 'LOCALAPPDATA']
    : platform.startsWith('macos-') ? ['HOME'] : []
  for (const name of nativeHomes) {
    const key = platform === 'windows' ? Object.keys(source).find(key => key.toUpperCase() === name) : name
    const value = source[key]
    const paths = platform === 'windows' ? path.win32 : path.posix
    if (source.MUNIMENT_NATIVE_DISPOSABLE_USER !== '1' || typeof value !== 'string' || !paths.isAbsolute(value)) {
      throw new Error(`The disposable native login requires an absolute ${name} path.`)
    }
    env[name] = value
  }
  // CEF requires cache paths and root cache paths to use the same spelling.
  fs.mkdirSync(root, { recursive: true, mode: 0o700 })
  root = fs.realpathSync(root)
  for (const name of ['PATH', 'SystemRoot', 'WINDIR', 'COMSPEC', 'PATHEXT', 'DISPLAY', 'WAYLAND_DISPLAY', 'DBUS_SESSION_BUS_ADDRESS', 'XAUTHORITY', 'LANG']) {
    if (source[name]) env[name] = source[name]
  }
  for (const [name, directory] of Object.entries({ HOME: 'home', USERPROFILE: 'home', APPDATA: 'roaming', LOCALAPPDATA: 'local',
    XDG_CONFIG_HOME: 'config', XDG_DATA_HOME: 'data', XDG_CACHE_HOME: 'cache', XDG_RUNTIME_DIR: 'runtime',
    TMPDIR: 'tmp', TEMP: 'tmp', TMP: 'tmp', MUNIMENT_STATE_DIR: 'state' })) {
    if (nativeHomes.includes(name)) continue
    env[name] = path.join(root, directory)
    fs.mkdirSync(env[name], { recursive: true, mode: 0o700 })
  }
  env.PI_CODING_AGENT_DIR = path.join(env.MUNIMENT_STATE_DIR, 'agent')
  fs.mkdirSync(env.PI_CODING_AGENT_DIR, { mode: 0o700 })
  env.MUNIMENT_SUBSCRIPTION_PROBE = '1'
  return env
}

export function prepareProbeHome(state) {
  // A fresh profile otherwise shows onboarding, whose Send only confirms Home.
  const home = path.join(state, 'probe-home')
  for (const directory of ['memory', 'agents', 'projects', 'sessions']) {
    fs.mkdirSync(path.join(home, directory), { recursive: true, mode: 0o700 })
  }
  save(path.join(state, 'home.json'), { location: home })
}

export function disposableProfilePrefix(platform, temporaryDirectory = os.tmpdir()) {
  // macOS GUI temporary directories can exceed the attach socket path limit.
  return path.join(platform.startsWith('macos-') ? '/tmp' : temporaryDirectory, 'muniment-subscriptions-')
}

export function assertAttachSocketPath(platform, state) {
  if (!platform.startsWith('macos-')) return
  const bytes = Buffer.byteLength(path.posix.join(state, 'muniment', 'attach-v1.sock'))
  // The 104-byte sun_path field must also hold the null terminator.
  if (bytes >= 104) throw new Error(`The macOS attach socket path uses ${bytes} bytes. The maximum is 103 bytes.`)
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
export function equalPayload(expected, installed) {
  const keys = Object.keys(expected)
  const missing = keys.filter(key => !Object.hasOwn(installed, key))
  const extra = Object.keys(installed).filter(key => !Object.hasOwn(expected, key))
  const differing = keys.filter(key => Object.hasOwn(installed, key) && installed[key] !== expected[key])
  if (!keys.length || missing.length || extra.length || differing.length) {
    throw new Error(`The installed payload does not match the signed package.\n${payloadDifferenceDetail({ missing, extra, differing })}`)
  }
}
function locate(root, name) {
  const matches = Object.keys(tree(root)).filter(file => path.basename(file) === name)
  if (matches.length !== 1) throw new Error(`The package does not contain one desktop executable. The package contains these matches: ${JSON.stringify(matches)}.`)
  return path.join(root, matches[0])
}

export function verifyInstalled(candidate, packageFile, executable, root, env, step = () => {}, executeNative = execute) {
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
    step('payload/macos-list')
    const names = executeNative('tar', ['-tzf', packageFile], env).split('\n')
    if (names.some(name => path.posix.isAbsolute(name) || name.split('/').includes('..'))) throw new Error('The package paths are invalid.')
    step('payload/macos-extract')
    executeNative('tar', ['-xzf', packageFile, '-C', expanded], env)
    payload = path.join(expanded, 'muniment.app')
    installed = path.resolve(executable, '../../..')
    if (path.relative(installed, executable) !== path.join('Contents', 'MacOS', 'muniment-desktop')) {
      throw new Error('Select the installed desktop inside the signed app bundle.')
    }
    step('payload/codesign')
    executeNative('codesign', ['--verify', '--deep', '--strict', installed], env)
    step('payload/gatekeeper')
    executeNative('spctl', ['--assess', '--type', 'execute', installed], env)
  } else {
    if (!candidate.asset.endsWith('_x64_en-US.msi')) throw new Error('Use the signed per-user Windows MSI.')
    step('payload/msi-admin-extract')
    executeNative('msiexec.exe', ['/a', packageFile, '/qn', '/L*v', path.join(env.TMPDIR, 'subscription-msi-admin.log'), `TARGETDIR=${expanded}`], env)
    step('payload/locate-executable')
    payload = path.dirname(locate(expanded, 'muniment-desktop.exe'))
    installed = path.dirname(executable)
    step('payload/authenticode')
    executeNative('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
      '$signature = Get-AuthenticodeSignature -LiteralPath $env:MUNIMENT_SUBSCRIPTION_PACKAGE; Write-Output $signature.Status; Write-Output $signature.StatusMessage; if ($signature.Status -ne "Valid") { exit 1 }'],
    { ...env, MUNIMENT_SUBSCRIPTION_PACKAGE: packageFile })
  }
  step('payload/compare')
  const expected = tree(payload)
  const before = tree(installed)
  equalPayload(expected, before)
  return () => {
    const after = tree(installed)
    equalPayload(expected, after)
    if (JSON.stringify(before) !== JSON.stringify(after)) throw new Error('The installed payload changed during the probe.')
  }
}

export const screenshotReasons = {
  'permission-denied': 'The macOS capture probe lacks Screen Recording permission. Grant Screen Recording to the desktop-ci capture process in the disposable GUI login.',
  'window-unavailable': 'The macOS screenshot requires one stable, visible installed app window. Restore the disposable GUI session. Rerun the check.',
  'window-uncapturable': 'The macOS window has no capturable image despite a granted permission check. Provide a desktop-ci display with working window backing images.',
  'diagnostics-unavailable': 'The macOS capture diagnostics failed. Provide Xcode command-line tools and a readable window list in the disposable GUI login.',
  'capture-failed': 'The macOS capture command failed. Check the native capture log in the platform artifact.',
}

export class ScreenshotError extends Error {
  constructor(code, detail) {
    const reason = screenshotReasons[code] ?? screenshotReasons['diagnostics-unavailable']
    super(`${reason}\ncapture=${code}\n${detail}`)
    this.reason = reason
  }
}

export function macosCaptureInfo(text, pid) {
  const info = JSON.parse(text)
  if (!Number.isInteger(pid) || pid <= 0 || pid > 2147483647 ||
      typeof info?.screen_capture_access !== 'boolean' || !Array.isArray(info.windows) ||
      info.windows.some(window => !window || !Number.isInteger(window.id) || window.id <= 0 || window.id > 0xffffffff ||
        window.pid !== pid || window.layer !== 0 || window.onscreen !== true ||
        ![window.x, window.y, window.width, window.height].every(Number.isFinite) || window.width < 1 || window.height < 1 ||
        ![-1, 0, 1, 2].includes(window.sharing_state)) ||
      new Set(info.windows.map(window => window.id)).size !== info.windows.length) {
    throw new Error('The macOS capture diagnostics are invalid.')
  }
  // Keep diagnostics numeric. Window titles and owner names can contain user data.
  return { screen_capture_access: info.screen_capture_access,
    windows: info.windows.map(({ id, pid, layer, onscreen, x, y, width, height, sharing_state }) =>
      ({ id, pid, layer, onscreen, x, y, width, height, sharing_state })) }
}

export function screenshot(platform, pid, output, env, executeNative = execute) {
  if (platform === 'linux') {
    const ids = execute('xdotool', ['search', '--onlyvisible', '--pid', String(pid), '--name', '^muniment$'], env, 10_000).split(/\s+/)
    if (ids.length !== 1 || !/^\d+$/.test(ids[0])) throw new Error('The native screenshot requires one installed app window.')
    execute('import', ['-window', ids[0], output], env, 10_000)
  } else if (platform.startsWith('macos-')) {
    const helper = path.join(env.TMPDIR, 'window-id')
    const inspect = () => macosCaptureInfo(executeNative(helper, [String(pid), '--capture-info'], env, 10_000), pid)
    let before
    try {
      executeNative('clang', ['-std=gnu17', '-framework', 'CoreFoundation', '-framework', 'CoreGraphics',
        '-o', helper, 'test/e2e/support/macos-window-count.c'], env)
      before = inspect()
    } catch (error) {
      throw new ScreenshotError('diagnostics-unavailable', error.message)
    }
    if (before.windows.length !== 1) {
      throw new ScreenshotError(before.screen_capture_access ? 'window-unavailable' : 'permission-denied',
        `before=${JSON.stringify(before)}`)
    }
    const id = before.windows[0].id
    try {
      // A helper's permission check must not veto a working screencapture process.
      executeNative('screencapture', ['-x', `-l${id}`, output], env, 10_000)
    } catch (error) {
      let after
      const imageFailure = !error.cause && !error.signal && error.message.includes('could not create image from window')
      const detail = `before=${JSON.stringify(before)}\n${error.message}`
      try { after = inspect() }
      catch (diagnosticError) {
        throw new ScreenshotError(imageFailure ? 'diagnostics-unavailable' : 'capture-failed', `${detail}\n${diagnosticError.message}`)
      }
      const code = !imageFailure ? 'capture-failed'
        : !after.screen_capture_access ? 'permission-denied'
          : after.windows.length !== 1 || after.windows[0].id !== id ? 'window-unavailable' : 'window-uncapturable'
      throw new ScreenshotError(code, `${detail}\nafter=${JSON.stringify(after)}`)
    }
  } else {
    execute('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.resolve('test/e2e/support/subscription-screenshot.ps1'),
      '-AppPid', String(pid), '-Destination', output], env, 10_000)
  }
}

export async function removeProbeProfile(root, { remove = fs.promises.rm, now = Date.now, wait = delay, timeout = 15_000 } = {}) {
  const deadline = now() + timeout
  for (;;) {
    try { await remove(root, { recursive: true, force: true }); return }
    catch (error) {
      if (!['EPERM', 'EBUSY', 'ENOTEMPTY', 'EACCES'].includes(error.code) || now() >= deadline) throw error
      await wait(Math.min(250, deadline - now()))
    }
  }
}

export async function awaitProbeResult(readResult, alive, { now = Date.now, wait = delay, timeout = 20 * 60_000 } = {}) {
  const deadline = now() + timeout
  while (now() < deadline) {
    const result = readResult()
    if (result) return result
    if (!alive()) throw new Error('The installed probe process exited before the result.')
    await wait(250)
  }
  throw new Error('The installed probe timed out before the result.')
}

export async function awaitUpdateResult(readResult, { now = Date.now, wait = delay, timeout = 300_000 } = {}) {
  const deadline = now() + timeout
  while (now() < deadline) {
    const result = readResult()
    if (result) return result
    await wait(250)
  }
  throw new Error('The updated app did not restart.')
}

export function verifyUpdateResult(result, parentPid, sourceSha, turns, verifyPayload) {
  const checks = featureChecks['signed-update'].slice(0, -1)
  if (result?.passed !== true || result.phase !== 'update-restart' || !Number.isSafeInteger(result.pid) ||
      result.pid <= 0 || result.pid > 2147483647 || result.pid === parentPid || result.update_parent_pid !== parentPid ||
      result.source_sha !== sourceSha || result.webdriver !== false ||
      JSON.stringify(result.turns) !== JSON.stringify(turns) ||
      JSON.stringify(result.features?.['signed-update']) !== JSON.stringify(checks)) {
    throw new Error('The installed update or profile restore failed.')
  }
  verifyPayload()
  result.features['signed-update'].push('candidate-digest-verified')
  return result
}

export function verifyMcpReceipt(checks, file, nonce) {
  if (checks?.[0] === 'failed') return checks
  try {
    if (json(file).token === nonce) return checks
  } catch { /* A missing or incomplete receipt fails the feature, not the whole probe. */ }
  return ['failed', 'receipt', 'check-failed']
}

export const updaterPublicKeyFile = 'src-tauri/updater.pub'

export function installedExecutable(platform, executable) {
  // Tauri caches the startup path before main and rejects macOS symlink ancestors.
  // Resolve /var and /tmp before launch so both the updater and restart use the real bundle.
  return ['macos-arm64', 'macos-x64'].includes(platform) ? fs.realpathSync(executable) : executable
}

export function writeBlocked(output, sourceSha, platform, reason, detail = '', redact = subscriptionRedactor(), failureKind, condition, featureFailures = {}) {
  fs.mkdirSync(output, { recursive: true, mode: 0o700 })
  const proof = blocked(sourceSha, platform, reason, featureFailures)
  const features = Object.fromEntries(proof.cases.filter(item => item.failure_stage)
    .map(item => [item.feature, featureFailures[item.feature]]))
  save(path.join(output, 'release-acceptance.json'), proof)
  save(path.join(output, `${platform}-subscription.json`), { status: failureKind === 'product' ? 'failed' : 'blocked', reason,
    ...(failureKind ? { failure_kind: failureKind } : {}), ...(condition ? { condition } : {}),
    ...(Object.keys(features).length ? { features } : {}) })
  const log = path.join(output, `${platform}-subscription.log`)
  const previous = fs.existsSync(log) ? fs.readFileSync(log, 'utf8') : ''
  const status = failureKind === 'product' ? 'failed' : 'blocked'
  fs.writeFileSync(log, redact(`${previous}The native acceptance run ${status === 'failed' ? 'failed' : 'is blocked'}.\n${detail}\n`), { mode: 0o600 })
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
  fs.rmSync(path.join(output, `${platform}-subscription.log`), { force: true })
  writeBlocked(output, sourceSha, platform, reason)
  let redact = subscriptionRedactor()
  let step = 'prerequisites', phase = 'not started'
  const setStep = value => { step = value }
  let root, env, app, appClosed, runtime, runtimeClosed, verify, candidate, packageName, updateServer, relaunchedPid
  const stop = async (child, closed) => {
    if (!child?.pid) return
    if (process.platform === 'win32') {
      await stopWindowsTree(child)
      return
    } else {
      try { process.kill(-child.pid, 'SIGKILL') } catch (error) { if (error.code !== 'ESRCH') throw error }
    }
    let timer
    try {
      await Promise.race([closed, new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error('The native probe process did not stop.')), 5000)
      })])
    } finally { clearTimeout(timer) }
  }
  let passed = false, failureKind, condition
  const featureFailures = {}
  const reportBlocked = (detail = '') => writeBlocked(output, sourceSha, platform, reason, detail, redact, failureKind, condition, featureFailures)
  const checkpointFailures = result => {
    for (const feature of Object.keys(featureChecks)) {
      const failure = featureFailure(feature, result?.features?.[feature])
      if (failure.failure_stage && !featureFailures[feature]) {
        featureFailures[feature] = [...result.features[feature]]
      }
    }
    // Keep only validated failure codes and counts until every phase and cleanup check passes.
    reportBlocked()
  }
  let evidence, proof, finalDiagnostics
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
    // Load the redaction context before any parser or native tool can fail.
    redact = subscriptionRedactor({ leases: fs.readFileSync(leasesFile, 'utf8') })
    executable = installedExecutable(platform, executable)
    step = 'candidate-identity'
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
    step = 'leases'
    const accounts = subscriptionAccounts(candidate.models, json(leasesFile))
    step = 'profile'
    reason = 'The disposable profile could not be created.'
    root = fs.realpathSync(fs.mkdtempSync(disposableProfilePrefix(platform)))
    env = isolatedEnvironment(root)
    step = 'profile/attach-socket-path'
    reason = 'The macOS attach socket path must be shorter than 104 bytes. Use a shorter disposable profile path.'
    assertAttachSocketPath(platform, env.MUNIMENT_STATE_DIR)
    reason = 'The installed payload does not match the signed package. Check native package and signature tools.'
    step = 'payload'
    const verifyPayload = verifyInstalled(candidate, packageFile, executable, root, env, setStep)
    verify = () => {
      verifyPayload()
      if (hash(fs.readFileSync(packageFile)) !== candidate.sha256) throw new Error('The candidate package changed during the probe.')
    }
    const nonce = `MUNIMENT-${randomBytes(16).toString('hex')}`
    const state = env.MUNIMENT_STATE_DIR
    prepareProbeHome(state)
    const fixtureFile = path.join(root, 'acceptance.txt')
    const fileNonce = `MUNIMENT-${randomBytes(16).toString('hex')}`
    const mcpNonce = `MUNIMENT-${randomBytes(16).toString('hex')}`
    const mcpReceipt = path.join(root, 'mcp-receipt.json')
    fs.writeFileSync(fixtureFile, fileNonce, { mode: 0o600 })
    const version = comment.split('\t').find(field => field.startsWith('version:'))?.slice(8)
    if (!/^\d+\.\d+\.\d+$/.test(version ?? '')) throw new Error('The signed package version is missing.')
    reason = 'The disposable signed update server could not start. Install OpenSSL on the native runner.'
    step = 'update-server'
    updateServer = await updateFixture(root, bytes, fs.readFileSync(signatureFile, 'utf8').trim(), version, platform)
    const plan = { platform, models: candidate.models, nonce, fileNonce, mcpNonce, mcpReceipt,
      acceptance: true, phase: 'chat', fixtureFile, fixtureDirectory: root,
      mcpCommand: process.execPath, mcpScript: path.resolve('test/e2e/support/subscription-mcp.mjs'),
      updateUrl: updateServer.url, packageSha256: candidate.sha256 }
    save(path.join(state, 'subscription-probe.json'), plan)
    fs.writeFileSync(path.join(state, '.adopted'), '', { mode: 0o600 })
    fs.writeFileSync(path.join(state, 'local-mode'), '1', { mode: 0o600 })
    save(path.join(env.PI_CODING_AGENT_DIR, 'muniment-router.json'), { enabled: true, accounts })
    save(path.join(env.PI_CODING_AGENT_DIR, 'auth.json'), {})
    save(path.join(env.PI_CODING_AGENT_DIR, 'settings.json'), {
      defaultProvider: 'muniment-router', defaultModel: `${candidate.models[0].family}/${candidate.models[0].id}`,
    })
    reason = 'The installed probe did not finish. Check the native runtime, subscription access, and selected models.'
    const resultFile = path.join(state, 'subscription-probe-result.json')
    const spawnLogged = async (file, args, name) => {
      const log = fs.openSync(path.join(env.TMPDIR, `subscription-${name}.log`), 'a', 0o600)
      try {
        if (platform === 'windows') return await launchWindowsTree(file, args, env, log, name === 'app' ? state : undefined)
        const child = spawn(file, args, { env, stdio: ['ignore', log, log], detached: true })
        child.once('error', error => {
          child.diagnosticError = error.message
        })
        return child
      } finally { fs.closeSync(log) }
    }
    const launch = async (updating = false) => {
      phase = json(path.join(state, 'subscription-probe.json')).phase
      step = `${phase}/runtime-start`
      if (platform !== 'linux') {
        const runtimeFile = platform === 'windows' ? path.join(path.dirname(executable), 'muniment-runtime.exe')
          : path.resolve(executable, '../../Library/LaunchServices/muniment-runtime')
        runtime = await spawnLogged(runtimeFile, [], 'runtime')
        runtimeClosed = new Promise(resolve => { runtime.once('exit', resolve); runtime.once('error', resolve) })
        await delay(3000)
        if (!runtime.pid || runtime.exitCode !== null || runtime.signalCode !== null ||
            (platform === 'windows' && !windowsTreeAlive(runtime))) throw new Error('The installed runtime could not start with the disposable profile.')
      }
      step = `${phase}/app-start`
      app = await spawnLogged(executable, ['--probe-subscription-chat', `--probe-subscription-profile=${state}`], 'app')
      let appError = false
      appClosed = new Promise(resolve => {
        app.once('exit', resolve)
        app.once('error', () => { appError = true; resolve() })
      })
      step = `${phase}/wait-result`
      if (updating) {
        const result = await awaitUpdateResult(() => fs.existsSync(resultFile) && json(resultFile))
        if (platform !== 'windows' && Number.isSafeInteger(result.pid) && result.pid > 0 && result.pid <= 2147483647 && result.pid !== app.pid) {
          relaunchedPid = result.pid
        }
        return result
      }
      return awaitProbeResult(() => fs.existsSync(resultFile) && json(resultFile),
        () => platform === 'windows' ? windowsTreeAlive(app) && (!runtime || windowsTreeAlive(runtime))
          : app.exitCode === null && app.signalCode === null && !appError &&
            (!runtime || (runtime.exitCode === null && runtime.signalCode === null)))
    }
    const probe = await launch()
    checkpointFailures(probe)
    step = 'chat/verify-result'
    reason = 'The installed subscription reply or model switch failed. Check model access and thread continuity.'
    if (probe.passed !== true) throw new AcceptanceError('probe-status')
    reason = 'The native model receipts or package identity failed verification. Check requested models, compiled source, and signed package.'
    // Freeze chat receipts before the feature launch adds provider requests.
    let receipts
    try {
      const text = fs.readFileSync(path.join(env.PI_CODING_AGENT_DIR, 'subscription-probe-transports.jsonl'), 'utf8').trim()
      receipts = text ? text.split('\n').map(JSON.parse) : []
    } catch { throw new AcceptanceError('chat-transport-file') }
    const transports = chatTransports(receipts, candidate.models[0].id)
    const chatResult = { status: 'passed', installed: true, unchanged: true, source_sha: probe.source_sha,
      webdriver: probe.webdriver, package_sha256: candidate.sha256,
      turns: Array.isArray(probe.turns) ? probe.turns.map(turn => ({ ...turn, requested: candidate.models[turn?.index]?.id, expected: nonce })) : probe.turns }
    acceptance(candidate, sourceSha, platform, chatResult, transports, packageName)
    verify()
    reason = 'The installed feature checks could not start with the disposable profile.'
    await stop(app, appClosed)
    await stop(runtime, runtimeClosed)
    app = runtime = undefined
    fs.rmSync(resultFile)
    save(path.join(state, 'subscription-probe.json'), { ...plan, phase: 'features', turns: probe.turns })
    const featureResult = await launch()
    checkpointFailures(featureResult)
    step = 'features/verify-result'
    if (!featureResult.passed || featureResult.source_sha !== probe.source_sha || featureResult.webdriver !== false ||
        JSON.stringify(featureResult.turns) !== JSON.stringify(probe.turns)) throw new Error(reason)
    featureResult.features.mcp = verifyMcpReceipt(featureResult.features.mcp, mcpReceipt, mcpNonce)
    checkpointFailures(featureResult)
    reason = 'The installed app could not restore its disposable profile after restart.'
    await stop(app, appClosed)
    await stop(runtime, runtimeClosed)
    app = runtime = undefined
    fs.rmSync(resultFile)
    save(path.join(state, 'subscription-probe.json'), { ...plan, phase: 'restart', turns: probe.turns })
    const restarted = await launch()
    checkpointFailures(restarted)
    step = 'restart/verify-result'
    if (!restarted.passed || restarted.source_sha !== probe.source_sha || restarted.webdriver !== false ||
        JSON.stringify(restarted.turns) !== JSON.stringify(probe.turns)) throw new Error(reason)
    verify()
    reason = 'The installed update failed or did not restore the disposable profile after relaunch.'
    await stop(app, appClosed)
    await stop(runtime, runtimeClosed)
    app = runtime = undefined
    fs.rmSync(resultFile)
    save(path.join(state, 'subscription-probe.json'), { ...plan, phase: 'update', turns: probe.turns })
    const updated = await launch(true)
    checkpointFailures(updated)
    step = 'update/verify-result'
    verifyUpdateResult(updated, app.probePid ?? app.pid, sourceSha, probe.turns, verify)
    // Require the relaunched process to stay alive through the native capture.
    process.kill(updated.pid, 0)
    const result = { status: 'passed', installed: true, unchanged: true, source_sha: probe.source_sha, webdriver: probe.webdriver,
      package_sha256: candidate.sha256, features: { ...probe.features, ...featureResult.features, ...restarted.features,
        'signed-update': updated.features['signed-update'], ...featureFailures },
      turns: probe.turns.map(turn => ({ ...turn, requested: candidate.models[turn.index]?.id, expected: nonce })) }
    proof = acceptance(candidate, sourceSha, platform, result, transports, packageName)
    step = 'screenshot'
    reason = 'The native screenshot failed. Install the capture tool and grant the disposable login screen capture access.'
    const raw = path.join(root, 'capture')
    const safe = path.join(root, 'safe')
    fs.mkdirSync(raw)
    const screenshotName = `screenshot-${platform}-subscriptions.png`
    screenshot(platform, updated.pid, path.join(raw, screenshotName), env)
    // Keep the redactor's injected-secret screening separate from the app environment.
    execute(process.execPath, ['test/e2e/support/redact.mjs', raw, safe], process.env)
    fs.copyFileSync(path.join(safe, screenshotName), path.join(output, screenshotName))
    evidence = { ...result, transports }
    passed = true
  } catch (error) {
    if (error instanceof AcceptanceError) condition = error.condition
    if (error instanceof ScreenshotError) reason = error.reason
    if (app && !(error instanceof ScreenshotError)) {
      failureKind = probeFailure(env)
      if (failureKind === 'auth') reason = 'Subscription authentication blocked the probe. Renew the factory access lease.'
      else if (failureKind === 'quota') reason = 'Subscription quota blocked the probe. Retry after capacity returns.'
    }
    reportBlocked(`step=${step}\nerror=${error.message}\napp: ${processStatus(app)}\nruntime: ${platform === 'linux' ? linuxRuntimeStatus(env) : processStatus(runtime)}\n${profileLogs(env, redact)}\n${probeProgress(env, step, phase)}`)
  } finally {
    let cleanupFailed = false
    if (relaunchedPid) {
      try {
        process.kill(relaunchedPid, 'SIGKILL')
      } catch (error) {
        if (error.code !== 'ESRCH') {
          cleanupFailed = true
          reportBlocked(`step=cleanup/relaunch\nerror=${error.message}`)
        }
      }
    }
    for (const [child, closed] of [[app, appClosed], [runtime, runtimeClosed]]) {
      try {
        await stop(child, closed)
      } catch (error) {
        cleanupFailed = true
        reportBlocked(`step=cleanup/stop\nerror=${error.message}`)
      }
    }
    try {
      verify?.()
    } catch (error) {
      cleanupFailed = true
      reportBlocked(`step=cleanup/payload\nerror=${error.message}`)
    }
    finalDiagnostics = redact(`app: ${processStatus(app)}\nruntime: ${platform === 'linux' ? linuxRuntimeStatus(env) : processStatus(runtime)}\n${profileLogs(env, redact)}\n${probeProgress(env, step, phase)}`)
    try {
      await updateServer?.close()
    } catch (error) {
      cleanupFailed = true
      reportBlocked(`step=cleanup/update-server\nerror=${error.message}`)
    }
    try {
      if (root) await removeProbeProfile(root)
    } catch (error) {
      cleanupFailed = true
      reportBlocked(`step=cleanup/profile\nerror=${error.message}`)
    }
    if (!passed || cleanupFailed) reportBlocked(finalDiagnostics)
    if (cleanupFailed) {
      passed = false
      failureKind = 'product'
      reason = `${reason} The native probe cleanup failed. Stop the disposable login before another check.`
      reportBlocked()
    }
  }
  if (passed) {
    save(evidenceFile, evidence)
    save(proofFile, proof)
    fs.writeFileSync(path.join(output, `${platform}-subscription.log`), proof.cases.map(item =>
      `${item.feature}: ${item.status}${item.failure_stage ? ` stage=${item.failure_stage} error=${item.error_class}` : ''}\n`).join('') + (proof.cases.some(item => item.status !== 'passed') ? finalDiagnostics : ''), { mode: 0o600 })
  }
  reportPayloadDifferences(fs.readFileSync(path.join(output, `${platform}-subscription.log`), 'utf8'), redact)
  return passed && proof.cases.every(item => item.status === 'passed') ? 0 : 1
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [candidateFile, packageFile, signatureFile, executable, leasesFile, output, sourceSha, platform] = process.argv.slice(2)
  if (!output || !/^[a-f0-9]{40}$/.test(sourceSha ?? '') || !platforms.includes(platform)) {
    console.error('Use subscriptions.mjs CANDIDATE PACKAGE SIGNATURE INSTALLED_APP LEASES OUTPUT SOURCE_SHA PLATFORM.')
    process.exitCode = 1
  } else process.exitCode = await run({ candidateFile, packageFile, signatureFile, executable, leasesFile, output, sourceSha, platform })
}
