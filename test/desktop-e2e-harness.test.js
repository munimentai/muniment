import { afterEach, describe, expect, it } from 'vitest'
import { execFileSync, spawn, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const root = process.cwd()
const temporary = []
const temp = () => { const value = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-e2e-test-')); temporary.push(value); return value }
afterEach(() => { for (const value of temporary.splice(0)) fs.rmSync(value, { recursive: true, force: true }) })
const runNode = (script, args, options = {}) => spawnSync(process.execPath, [path.join(root, script), ...args], { encoding: 'utf8', ...options })

describe('installed onboarding spec contract', () => {
  it.each(['onboarding.spec.js', 'real-sign-in.spec.js'])('%s uses only shipped onboarding controls', (name) => {
    const spec = fs.readFileSync(path.join(root, 'test/e2e/specs', name), 'utf8')
    const selectors = [...spec.matchAll(/data-testid=(["'])(onboarding-[^"']+)\1/g)].map((match) => match[2])
    expect(selectors.length).toBeGreaterThan(0)
    expect(new Set(selectors)).toEqual(new Set(['onboarding-home-path', 'onboarding-picker', 'onboarding-confirm']))
  })
})

describe('WDIO Tauri service dependency contract', () => {
  it('loads the installed ESM entry with compatible transitive named exports', async () => {
    await expect(import('@wdio/tauri-service')).resolves.toBeDefined()
  }, 15_000)
})

describe('nightly asset identity', () => {
  const sha = 'a'.repeat(40)
  const asset = { name: `nightly-${sha}-linux-muniment.deb`, id: 42 }
  const validate = (release, candidate = sha, platform = 'linux') => runNode('test/e2e/support/asset-identity.mjs', [candidate, platform], { input: JSON.stringify(release) })
  it('accepts exactly one pinned asset', () => expect(validate({ target_commitish: sha, assets: [asset] }).stdout).toBe('42'))
  it.each([
    ['missing', { target_commitish: sha, assets: [] }],
    ['duplicate', { target_commitish: sha, assets: [asset, asset] }],
  ])('rejects %s identity', (_name, release) => expect(validate(release).status).not.toBe(0))
  it('rejects a noncanonical SHA', () => expect(validate({ target_commitish: sha, assets: [asset] }, 'A'.repeat(40)).status).not.toBe(0))
  it('accepts only the per-user Windows MSI', () => {
    const perUser = { name: `nightly-${sha}-windows-muniment_0.0.1_x64_en-US.msi`, id: 84 }
    const machine = { name: `nightly-${sha}-windows-muniment-machine.msi`, id: 85 }
    expect(validate({ target_commitish: sha, assets: [perUser, machine] }, sha, 'windows').stdout).toBe('84')
  })
  it('accepts exactly the pinned macOS app archive', () => {
    const app = { name: `nightly-${sha}-macos-muniment.app.zip`, id: 126 }
    expect(validate({ target_commitish: sha, assets: [app] }, sha, 'macos').stdout).toBe('126')
    expect(validate({ target_commitish: sha, assets: [app, app] }, sha, 'macos').status).not.toBe(0)
  })
  it.each([
    `nightly-${sha}-macos-muniment.zip`,
    `nightly-${sha}-macos-other.app.zip`,
    `nightly-${sha}-macos-muniment.app.zip.extra`,
  ])('rejects unrelated or malformed macOS archive %s', (name) => {
    expect(validate({ target_commitish: sha, assets: [{ name, id: 126 }] }, sha, 'macos').status).not.toBe(0)
  })
  it('rejects duplicate Windows per-user assets', () => {
    const perUser = { name: `nightly-${sha}-windows-muniment_0.0.1_x64_en-US.msi`, id: 84 }
    expect(validate({ target_commitish: sha, assets: [perUser, perUser] }, sha, 'windows').status).not.toBe(0)
  })
  it.each([
    `nightly-${sha}-windows-unrelated.msi`,
    `nightly-${sha}-windows-muniment.msi`,
    `nightly-${sha}-windows-muniment_1.2_x64_en-US.msi`,
    `nightly-${sha}-windows-muniment_1.2.3_arm64_en-US.msi`,
    `nightly-${sha}-windows-muniment_1.2.3_x64_en-US.msi.zip`,
  ])('rejects unrelated or malformed Windows MSI %s', (name) => {
    expect(validate({ target_commitish: sha, assets: [{ name, id: 84 }] }, sha, 'windows').status).not.toBe(0)
  })
})

describe('Windows auth URL capture seam', () => {
  const capture = (candidate, initial = undefined) => {
    const directory = temp(); const destination = path.join(directory, 'auth-url')
    if (initial !== undefined) fs.writeFileSync(destination, initial)
    const result = runNode('test/e2e/support/capture-auth-url.mjs', [destination, candidate])
    return { result, destination }
  }
  it('captures an HTTPS URL without adding a BOM', () => {
    const candidate = 'https://auth.example.test/sign-in?state=abc%20123'
    const { result, destination } = capture(candidate)
    expect(result.status).toBe(0)
    expect(fs.readFileSync(destination)).toEqual(Buffer.from(candidate))
  })
  it.each(['http://auth.example.test', 'not a URL', 'https://user:secret@auth.example.test', 'https://auth.example.test\nsecond'])('rejects unsafe URL %s without replacing the destination', (candidate) => {
    const { result, destination } = capture(candidate, 'previous')
    expect(result.status).not.toBe(0)
    expect(fs.readFileSync(destination, 'utf8')).toBe('previous')
  })
  it('keeps browser-launcher.ps1 as a test-only delegate to the validated capture helper', () => {
    const launcher = fs.readFileSync(path.join(root, 'test/e2e/support/browser-launcher.ps1'), 'utf8')
    expect(launcher).toContain('capture-auth-url.mjs')
    expect(launcher).toContain('MUNIMENT_E2E_AUTH_URL_FILE')
  })
  it.skipIf(process.platform !== 'win32')('invokes browser-launcher.ps1 and preserves an existing capture on rejection', () => {
    const directory = temp(); const destination = path.join(directory, 'auth-url')
    const invoke = (candidate) => spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(root, 'test/e2e/support/browser-launcher.ps1'), candidate], {
      encoding: 'utf8', env: { ...process.env, MUNIMENT_E2E_AUTH_URL_FILE: destination },
    })
    expect(invoke('https://auth.example.test/sign-in?state=abc').status).toBe(0)
    expect(fs.readFileSync(destination, 'utf8')).toBe('https://auth.example.test/sign-in?state=abc')
    expect(invoke('http://auth.example.test/sign-in').status).not.toBe(0)
    expect(fs.readFileSync(destination, 'utf8')).toBe('https://auth.example.test/sign-in?state=abc')
    expect(invoke('https://user:secret@auth.example.test/sign-in').status).not.toBe(0)
    expect(fs.readFileSync(destination, 'utf8')).toBe('https://auth.example.test/sign-in?state=abc')
    const missingDestination = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(root, 'test/e2e/support/browser-launcher.ps1'), 'https://auth.example.test'], {
      encoding: 'utf8', env: { ...process.env, MUNIMENT_E2E_AUTH_URL_FILE: '' },
    })
    expect(missingDestination.status).not.toBe(0)
  })
})

describe('macOS installed launch harness', () => {
  const runnerPath = path.join(root, 'test/e2e/runner/macos.sh')
  const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/macos.sh'), 'utf8')
  const finalizer = runner.slice(runner.indexOf('finalize()'), runner.indexOf('\nif [[ ${MUNIMENT_E2E_FINALIZER_TEST_MODE'))
  const finalizerPhases = [...finalizer.matchAll(/cleanup_step ([a-z-]+)/g)].map((match) => match[1])

  const macosFixture = (failed = '') => {
    const directory = temp(); const ledger = path.join(directory, 'ledger'); const statusLedger = path.join(directory, 'status-ledger'); const artifacts = path.join(directory, 'artifacts')
    const env = { ...process.env, TMPDIR: directory, DCI_ARTIFACTS_DIR: artifacts, MUNIMENT_E2E_FINALIZER_TEST_MODE: '1', MUNIMENT_E2E_FINALIZER_TEST_LEDGER: ledger, MUNIMENT_E2E_FINALIZER_TEST_STATUS_LEDGER: statusLedger, MUNIMENT_E2E_FINALIZER_TEST_FAIL: failed }
    const read = (file) => fs.existsSync(file) ? fs.readFileSync(file, 'utf8').trim().split(/\r?\n/).filter(Boolean) : []
    const outcome = (result) => ({ result, invoked: read(ledger).map((line) => line.split('\t')[0]), statuses: Object.fromEntries(read(statusLedger).map((line) => line.split('\t'))) })
    return { directory, env, outcome }
  }

  const runMacosFinalizer = (failed = '') => {
    const fixture = macosFixture(failed)
    return fixture.outcome(spawnSync('bash', [runnerPath], { encoding: 'utf8', env: fixture.env }))
  }

  const signalMacosFinalizer = (signal) => new Promise((resolve, reject) => {
    const fixture = macosFixture(); const ready = path.join(fixture.directory, 'ready')
    const child = spawn('bash', [runnerPath], { env: { ...fixture.env, MUNIMENT_E2E_FINALIZER_TEST_READY: ready }, stdio: 'ignore' })
    const deadline = Date.now() + 5000
    const poll = setInterval(() => {
      if (fs.existsSync(ready)) {
        clearInterval(poll)
        child.kill(signal)
      } else if (Date.now() >= deadline) {
        clearInterval(poll); child.kill('SIGKILL'); reject(new Error('macOS finalizer fixture did not become ready'))
      }
    }, 10)
    child.once('error', reject)
    child.once('close', (code, receivedSignal) => resolve(fixture.outcome({ status: code, signal: receivedSignal })))
  })

  it('uses the native metadata-preserving install and bounded visible-window probe', () => {
    expect(runner).toContain('ditto -x -k "$archive" "$expanded"')
    expect(runner).toContain('ditto "$source_bundle" "$installed_bundle"')
    expect(runner).toContain("Print :CFBundleExecutable")
    expect(runner).toContain("stat -f '%Su' /dev/console")
    expect(runner).toContain('window_deadline=$((SECONDS + 60))')
    expect(runner).toContain('with timeout of 2 seconds')
    expect(runner).toContain('whose visible is true')
    expect(runner).toContain('screendump=requested-by-desktop-ci')
  })

  it('contains no sign-in or WebDriver automation', () => {
    expect(runner).not.toMatch(/MUNIMENT_E2E_(?:USERNAME|PASSWORD)/)
    expect(runner).not.toMatch(/wdio|tauri-driver/i)
  })

  it('cleans processes, the installed bundle, and state before redaction and publication', () => {
    const phases = finalizerPhases
    expect(phases.slice(0, 6)).toEqual(['stop-app', 'remove-bundle', 'remove-state', 'bundle-gone', 'processes-gone', 'state-gone'])
    expect(phases.indexOf('processes-gone')).toBeLessThan(phases.indexOf('redact-artifacts'))
    expect(phases.indexOf('redact-artifacts')).toBeLessThan(phases.indexOf('publish-artifacts'))
    expect(finalizer).toContain('suppress-artifacts')
  })

  it('runs every finalizer phase and succeeds after a normal smoke', () => {
    const { result, invoked } = runMacosFinalizer()
    expect(result.status).toBe(0)
    expect(invoked).toEqual(finalizerPhases.filter((phase) => phase !== 'suppress-artifacts'))
  })

  it.each(['SIGTERM', 'SIGINT'])('fails closed and finalizes after %s', async (signal) => {
    const { result, invoked } = await signalMacosFinalizer(signal)
    expect(result.status).not.toBe(0)
    expect(result.signal).toBeNull()
    expect(invoked).toEqual(finalizerPhases.filter((phase) => phase !== 'suppress-artifacts'))
    expect(invoked).toEqual(expect.arrayContaining(['stop-app', 'remove-bundle', 'remove-state', 'processes-gone', 'redact-artifacts']))
  }, 15_000)

  it.each(['stop-app', 'remove-bundle', 'remove-state', 'processes-gone', 'redact-artifacts', 'replace-artifacts', 'publish-artifacts'])(
    'continues cleanup and fails after injected %s failure', (failed) => {
      const { result, invoked, statuses } = runMacosFinalizer(failed)
      expect(result.status).not.toBe(0)
      expect(statuses[failed]).toBe('1')
      expect(invoked).toContain('redact-artifacts')
      expect(invoked).toContain('remove-cleanup-log')
      if (failed === 'redact-artifacts') expect(invoked).toContain('suppress-artifacts')
      else if (!['replace-artifacts', 'publish-artifacts'].includes(failed)) expect(invoked).toContain('publish-artifacts')
      else expect(invoked).toContain('suppress-artifacts')
    },
  )
})

describe('Windows finalizer contract', () => {
  const runnerPath = path.join(root, 'test/e2e/runner/windows.ps1')
  const runner = fs.readFileSync(runnerPath, 'utf8')
  const finalizer = runner.slice(runner.indexOf('function Finalize-Run'), runner.indexOf('\ntry {'))
  const phases = [...finalizer.matchAll(/Invoke-Cleanup "([^"]+)"/g)].map((match) => match[1])
  const injectablePhases = [...new Set(phases)].filter((phase) => phase !== 'suppress-artifacts')

  it('asserts installer registration and files before generic state removal', () => {
    expect(phases.indexOf('uninstall')).toBeLessThan(phases.indexOf('registration-gone'))
    expect(phases.indexOf('registration-gone')).toBeLessThan(phases.indexOf('installed-files-gone'))
    expect(phases.indexOf('installed-files-gone')).toBeLessThan(phases.indexOf('remove-state'))
    expect(finalizer).toMatch(/registration-gone[^\n]+Get-ProductRegistration/)
    expect(finalizer).toMatch(/installed-files-gone[^\n]+Test-Path -LiteralPath \$installDirectory/)
    expect(finalizer).toMatch(/processes-gone[\s\S]+Get-HarnessProcesses/)
    expect(runner).toMatch(/function Get-HarnessProcesses[\s\S]+Get-Process muniment, tauri-driver, msedgedriver/)
    expect(finalizer).toMatch(/publicationStatus = \$cleanupStatus[\s\S]+cleanupStatus -ne \$publicationStatus[\s\S]+suppress-artifacts/)
  })

  it('establishes try/finally before directory and cleanup-log creation', () => {
    const boundary = runner.indexOf('\ntry {')
    expect(runner.indexOf('New-Item -ItemType Directory -Force $raw, $stateRoot')).toBeGreaterThan(boundary)
    expect(runner.indexOf('New-Item -ItemType File -Force $cleanupLog')).toBeGreaterThan(boundary)
    expect(runner).toMatch(/finally \{\s*Finalize-Run\s*\}/)
  })

  const runWindowsFinalizer = (failed = '', setupFail = '', extraEnv = {}) => {
    const directory = temp(); const ledger = path.join(directory, 'ledger'); const statusLedger = path.join(directory, 'status-ledger'); const artifacts = path.join(directory, 'artifacts')
    fs.mkdirSync(artifacts)
    fs.writeFileSync(path.join(artifacts, 'stale-or-partial'), 'unsafe')
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', runnerPath], {
      encoding: 'utf8',
      env: { ...process.env, TEMP: directory, TMP: directory, DCI_ARTIFACTS_DIR: artifacts, MUNIMENT_E2E_FINALIZER_TEST_MODE: '1', MUNIMENT_E2E_FINALIZER_TEST_LEDGER: ledger, MUNIMENT_E2E_FINALIZER_TEST_STATUS_LEDGER: statusLedger, MUNIMENT_E2E_FINALIZER_TEST_FAIL: failed, MUNIMENT_E2E_FINALIZER_TEST_SETUP_FAIL: setupFail, ...extraEnv },
    })
    const statuses = fs.existsSync(statusLedger) ? Object.fromEntries(fs.readFileSync(statusLedger, 'utf8').trim().split(/\r?\n/).map((entry) => entry.split('\t'))) : {}
    return { result, artifacts, directory, statuses, invoked: fs.existsSync(ledger) ? fs.readFileSync(ledger, 'utf8').trim().split(/\r?\n/) : [] }
  }

  const runWindowsAbsenceFailure = (variable) => runWindowsFinalizer('', '', { [variable]: '1' })

  it.skipIf(process.platform !== 'win32')('passes all lifecycle absence actions after fixture uninstall', () => {
    const { result, statuses } = runWindowsFinalizer()
    expect(result.status).toBe(0)
    expect(statuses['registration-gone']).toBe('0')
    expect(statuses['installed-files-gone']).toBe('0')
    expect(statuses['processes-gone']).toBe('0')
  })

  it.skipIf(process.platform !== 'win32').each([
    ['MUNIMENT_E2E_FINALIZER_TEST_REMAIN_REGISTRATION', 'registration-gone'],
    ['MUNIMENT_E2E_FINALIZER_TEST_REMAIN_FILES', 'installed-files-gone'],
    ['MUNIMENT_E2E_FINALIZER_TEST_REMAIN_PROCESS', 'processes-gone'],
  ])('fails the real %s fixture absence action and continues cleanup', (variable, phase) => {
    const { result, invoked, statuses } = runWindowsAbsenceFailure(variable)
    expect(result.status).not.toBe(0)
    expect(statuses[phase]).toBe('1')
    expect(invoked.indexOf('redact-artifacts')).toBeGreaterThan(invoked.indexOf(phase))
    expect(invoked).toContain('publish-artifacts')
  })

  it.skipIf(process.platform !== 'win32')('ignores a remaining unrelated uninstall registration', () => {
    const { result, statuses } = runWindowsFinalizer('', '', {
      MUNIMENT_E2E_FINALIZER_TEST_REMAIN_REGISTRATION: '1',
      MUNIMENT_E2E_FINALIZER_TEST_REGISTRATION_NAME: 'another product',
    })
    expect(result.status).toBe(0)
    expect(statuses['registration-gone']).toBe('0')
  })

  it.skipIf(process.platform !== 'win32').each(injectablePhases)('continues every Windows cleanup step after injected %s failure', (failed) => {
    const { result, invoked } = runWindowsFinalizer(failed)
    expect(result.status).not.toBe(0)
    expect(invoked).toContain(failed)
    for (const later of injectablePhases.slice(injectablePhases.indexOf(failed) + 1)) {
      if (failed === 'redact-artifacts' && later === 'publish-artifacts') continue
      expect(invoked).toContain(later)
    }
    if (['redact-artifacts', 'publish-artifacts'].includes(failed)) expect(invoked).toContain('suppress-artifacts')
  })

  it.skipIf(process.platform !== 'win32')('finalizes a failure during partial setup', () => {
    const { result, invoked } = runWindowsFinalizer('', 'before-directories')
    expect(result.status).not.toBe(0)
    expect(invoked).toContain('redact-artifacts')
    expect(invoked).toContain('suppress-artifacts')
    expect(invoked.at(-1)).toBe('suppress-artifacts')
  })

  it.skipIf(process.platform !== 'win32').each(['redact-artifacts', 'publish-artifacts'])('destroys raw/safe staging and suppresses publication after %s failure', (failed) => {
    const { result, artifacts, directory, invoked } = runWindowsFinalizer(failed)
    expect(result.status).not.toBe(0)
    expect(invoked).toContain('suppress-artifacts')
    expect(fs.existsSync(artifacts)).toBe(false)
    expect(fs.readdirSync(directory).filter((name) => name.startsWith('muniment-e2e-'))).toEqual([])
  })
})

describe('Windows nightly workflow gate', () => {
  const workflow = fs.readFileSync(path.join(root, '.github/workflows/nightly.yml'), 'utf8')
  const job = workflow.slice(workflow.indexOf('\n  windows-e2e:'), workflow.indexOf('\n    runs-on:', workflow.indexOf('\n  windows-e2e:')))
  const condition = job.match(/\n    if: >-\n([\s\S]+)$/)?.[1].trim().replace(/\n\s*/g, ' ')
  const evaluate = ({ eventName, platform, prepare = 'success', linux = 'success' }) => Function(
    'always', 'needs', 'github',
    `return ${condition.replaceAll('needs.linux-e2e', 'needs.linuxE2e')}`,
  )(() => true, { prepare: { result: prepare }, linuxE2e: { result: linux } }, { event_name: eventName, event: { inputs: { platform } } })

  it.each([
    ['schedule', undefined],
    ['workflow_dispatch', 'all'],
  ])('runs after Linux for a full %s nightly', (eventName, platform) => {
    expect(evaluate({ eventName, platform })).toBe(true)
  })

  it('excludes a Linux-only dispatch', () => {
    expect(evaluate({ eventName: 'workflow_dispatch', platform: 'linux' })).toBe(false)
  })

  it.each([
    ['workflow_dispatch', 'windows', 'success', 'skipped'],
    ['schedule', undefined, 'failure', 'success'],
    ['schedule', undefined, 'success', 'skipped'],
  ])('does not run without the full serialized prerequisites', (eventName, platform, prepare, linux) => {
    expect(evaluate({ eventName, platform, prepare, linux })).toBe(false)
  })
})

describe('artifact redaction boundary', () => {
  const redact = (files, env = {}) => {
    const source = temp(); const destination = path.join(temp(), 'safe')
    for (const [name, data] of Object.entries(files)) fs.writeFileSync(path.join(source, name), data)
    return { result: runNode('test/e2e/support/redact.mjs', [source, destination], { env: { ...process.env, ...env } }), destination }
  }
  it('emits ordinary diagnostics', () => {
    const { result, destination } = redact({ 'wdio.log': 'ordinary failure\n' })
    expect(result.status).toBe(0); expect(fs.readFileSync(path.join(destination, 'wdio.log'), 'utf8')).toContain('ordinary')
  })
  it.each([
    ['injected text', { 'app.log': 'private-user' }, { MUNIMENT_E2E_USERNAME: 'private-user' }],
    ['header token', { 'driver.log': 'Authorization: Bearer abcdefghijklmnopqrstuvwxyz' }, {}],
    ['unapproved screenshot', { 'failure-current-window.png': Buffer.from('not safe') }, {}],
  ])('blocks %s before destination creation', (_name, files, env) => {
    const { result, destination } = redact(files, env)
    expect(result.status).not.toBe(0); expect(fs.existsSync(destination)).toBe(false)
  })
  it('blocks an injected value hidden in an approved screenshot file', () => {
    const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=', 'base64')
    const { result, destination } = redact({ '01-signed-out.png': Buffer.concat([png, Buffer.from('private-user')]) }, { MUNIMENT_E2E_USERNAME: 'private-user' })
    expect(result.status).not.toBe(0); expect(fs.existsSync(destination)).toBe(false)
  })
})

describe('desktop-ci payload extraction', () => {
  const markers = (body) => `=== DESKTOP-CI ARTIFACTS BEGIN ===\n${body}\n=== DESKTOP-CI ARTIFACTS END ===\n`
  const archive = (setup) => {
    const source = temp(); setup(source)
    return execFileSync('tar', ['-czf', '-', '-C', source, '.']).toString('base64')
  }
  const extractInto = (output) => {
    const file = path.join(temp(), 'output'); const destination = path.join(temp(), 'artifacts'); fs.writeFileSync(file, output)
    const result = spawnSync('bash', [path.join(root, 'test/e2e/support/extract-artifacts.sh'), file, destination], { encoding: 'utf8' })
    return { result, destination }
  }
  const extract = (output) => extractInto(output).result
  const diagnostics = (destination) => Object.fromEntries(
    fs.readFileSync(path.join(destination, 'envelope-diagnostics.txt'), 'utf8').trim().split('\n').map((line) => line.split(/=(.*)/s).slice(0, 2)),
  )
  it('accepts one strict payload', () => expect(extract(markers(archive((dir) => fs.writeFileSync(path.join(dir, 'app.log'), 'safe')))).status).toBe(0))
  it.each(['', 'junk', `${markers('junk')}${markers('junk')}`, `=== DESKTOP-CI ARTIFACTS END ===\n=== DESKTOP-CI ARTIFACTS BEGIN ===\njunk\n`])('rejects malformed or ambiguous markers/base64', (value) => expect(extract(value).status).not.toBe(0))
  it('rejects link members', () => expect(extract(markers(archive((dir) => fs.symlinkSync('/tmp', path.join(dir, 'link'))))).status).not.toBe(0))
  it('rejects traversal members', () => {
    const source = temp(); fs.writeFileSync(path.join(source, 'file'), 'unsafe')
    const encoded = execFileSync('tar', ['-czf', '-', '--transform=s,^,../,', '-C', source, 'file']).toString('base64')
    expect(extract(markers(encoded)).status).not.toBe(0)
  })

  it('records a missing envelope as zero markers so the opaque nightly failure is diagnosable', () => {
    const { result, destination } = extractInto('desktop-ci setup log with no envelope\n')
    expect(result.status).not.toBe(0)
    const fields = diagnostics(destination)
    expect(fields.stage).toBe('markers')
    expect(fields.begin_marker_count).toBe('0')
    expect(fields.end_marker_count).toBe('0')
    expect(Number(fields.transcript_line_count)).toBeGreaterThan(0)
    expect(Number(fields.transcript_byte_count)).toBeGreaterThan(0)
  })

  it('records a duplicated envelope with its true marker counts', () => {
    const { result, destination } = extractInto(`${markers('junk')}${markers('junk')}`)
    expect(result.status).not.toBe(0)
    const fields = diagnostics(destination)
    expect(fields.stage).toBe('markers')
    expect(fields.begin_marker_count).toBe('2')
    expect(fields.end_marker_count).toBe('2')
  })

  it('distinguishes a corrupt payload from a marker fault', () => {
    const { result, destination } = extractInto(markers('not*valid*base64'))
    expect(result.status).not.toBe(0)
    const fields = diagnostics(destination)
    expect(fields.stage).toBe('base64')
    expect(fields.begin_marker_count).toBe('1')
    expect(fields.end_marker_count).toBe('1')
  })

  it('never leaks transcript content into the diagnostics', () => {
    const { destination } = extractInto('=== DESKTOP-CI ARTIFACTS BEGIN ===\ncorp-secret-value AKIA0123456789 Bearer sk-live-abcdefg\n')
    const raw = fs.readFileSync(path.join(destination, 'envelope-diagnostics.txt'), 'utf8')
    expect(raw).not.toContain('corp-secret-value')
    expect(raw).not.toContain('AKIA0123456789')
    expect(raw).not.toContain('sk-live-abcdefg')
  })
})

describe('cleanup failure accounting', () => {
  const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/linux.sh'), 'utf8')
  const phases = ['stop-wdio', 'stop-driver', 'revoke-session', 'stop-browser-driver', 'stop-app', 'remove-package', 'remove-state', 'package-gone', 'processes-gone', 'state-gone', 'stage-cleanup-log', 'redact-artifacts', 'remove-raw', 'remove-package-file', 'remove-auth-url', 'replace-artifacts', 'publish-artifacts', 'suppress-artifacts', 'remove-safe', 'raw-gone', 'package-file-gone', 'auth-url-gone', 'safe-gone', 'remove-cleanup-log']
  const runFinalizer = (failed = '', extraEnv = {}) => {
    const dir = temp(); const ledger = path.join(dir, 'ledger'); const statusLedger = path.join(dir, 'status-ledger')
    const result = spawnSync('bash', [path.join(root, 'test/e2e/runner/linux.sh')], {
      encoding: 'utf8',
      env: { ...process.env, MUNIMENT_E2E_FINALIZER_TEST_MODE: '1', MUNIMENT_E2E_FINALIZER_TEST_LEDGER: ledger, MUNIMENT_E2E_FINALIZER_TEST_STATUS_LEDGER: statusLedger, MUNIMENT_E2E_FINALIZER_TEST_FAIL: failed, ...extraEnv },
    })
    const entries = fs.readFileSync(ledger, 'utf8').trim().split('\n')
    const statuses = Object.fromEntries(fs.readFileSync(statusLedger, 'utf8').trim().split('\n').map((entry) => entry.split('\t')))
    return { result, entries, statuses, invoked: entries.map((entry) => entry.split('\t')[0]) }
  }
  const commands = (entries) => Object.fromEntries(entries.map((entry) => entry.split('\t', 2)))
  it('prefers the packaged binary name and retains the legacy fallback', () => {
    expect(runner).toContain('app_binary=$(command -v muniment-desktop || command -v muniment)')
  })
  it('clears stale automation before reaching recovery, then tears down the app', () => {
    const { result, entries, invoked } = runFinalizer()
    const command = commands(entries)
    expect(result.status).toBe(0)
    expect(invoked.slice(0, 5)).toEqual(['stop-wdio', 'stop-driver', 'revoke-session', 'stop-browser-driver', 'stop-app'])
    expect(command['stop-wdio']).toBe("stop_matching \\[w\\]dio.\\\*test/e2e/wdio.conf.js ")
    expect(command['stop-driver']).toBe("stop_matching \\[t\\]auri-driver ")
    expect(command['revoke-session']).toBe('timeout 45 env MUNIMENT_E2E_CLEANUP_ONLY=1 xvfb-run -a npm run test:e2e ')
    expect(command['stop-browser-driver']).toBe("stop_matching \\[c\\]hromedriver.\\\*9515 ")
    expect(command['stop-app']).toBe("bash -c pkill\\ -f\\ \\\'\\(\\^\\|/\\)muniment-desktop\\(\\ \\|\\\$\\)\\\'\\ 2\\\>/dev/null\\ \\|\\|\\ true\\\;\\ pkill\\ -x\\ muniment\\ 2\\\>/dev/null\\ \\|\\|\\ true\\\;\\ \\!\\ pgrep\\ -f\\ \\\'\\(\\^\\|/\\)muniment-desktop\\(\\ \\|\\\$\\)\\\'\\ \\>/dev/null\\ \\&\\&\\ \\!\\ pgrep\\ -x\\ muniment\\ \\>/dev/null ")
    expect(command['remove-package']).toBe('sudo apt-get remove -y muniment ')
    expect(command['package-gone']).toBe('package_absent ')
    expect(command['processes-gone']).toBe("bash -c \\!\\ pgrep\\ -f\\ \\\'\\(\\^\\|/\\)muniment-desktop\\(\\ \\|\\\$\\)\\\'\\ \\&\\&\\ \\!\\ pgrep\\ -x\\ muniment\\ \\&\\&\\ \\!\\ pgrep\\ -f\\ \\\'\\\[t\\\]auri-driver\\\'\\ \\&\\&\\ \\!\\ pgrep\\ -f\\ \\\'\\\[c\\\]hromedriver.\\\*9515\\\'\\ \\&\\&\\ \\!\\ pgrep\\ -f\\ \\\'\\\[w\\\]dio.\\\*test/e2e/wdio.conf.js\\\' ")

    const target = (label, operation) => {
      const match = command[label].match(new RegExp(`^${operation} ((?:/tmp/[^ ]+)) $`))
      expect(match, `${label} command and target`).not.toBeNull()
      return match[1]
    }
    const state = target('remove-state', 'rm -rf --')
    const raw = target('remove-raw', 'rm -rf --')
    const deb = target('remove-package-file', 'rm -f --')
    const auth = target('remove-auth-url', 'rm -f --')
    const safe = target('remove-safe', 'rm -rf --')
    expect(command['state-gone']).toBe(`cleanup_absent ${state} `)
    expect(command['raw-gone']).toBe(`cleanup_absent ${raw} `)
    expect(command['package-file-gone']).toBe(`cleanup_absent ${deb} `)
    expect(command['auth-url-gone']).toBe(`cleanup_absent ${auth} `)
    expect(command['safe-gone']).toBe(`cleanup_absent ${safe} `)
    expect(command['redact-artifacts']).toBe(`node test/e2e/support/redact.mjs ${raw} ${safe} `)
    expect(command['stage-cleanup-log']).toMatch(new RegExp(`^cp /tmp/muniment-e2e-cleanup\\.[^ ]+\\.log ${raw}/cleanup\\.log $`))
    const cleanupLog = command['stage-cleanup-log'].split(' ')[1]
    expect(command['replace-artifacts']).toBe('rm -rf -- /tmp/dci-artifacts ')
    expect(command['publish-artifacts']).toBe(`mv -- ${safe} /tmp/dci-artifacts `)
    expect(command['remove-cleanup-log']).toBe(`rm -f -- ${cleanupLog}`)
  })
  it.each([
    ['remove-package', 'package-gone'], ['remove-state', 'state-gone'], ['remove-raw', 'raw-gone'],
    ['remove-package-file', 'package-file-gone'], ['remove-auth-url', 'auth-url-gone'], ['remove-safe', 'safe-gone'],
  ])('fails %s absence verification when removal is unsuccessful', (removal, verification) => {
    const { result, entries, statuses } = runFinalizer(removal)
    expect(result.status).not.toBe(0)
    expect(commands(entries)[verification]).toMatch(/^(?:package_absent|cleanup_absent \/tmp\/)/)
    expect(statuses[removal]).toBe('1')
    expect(statuses[verification]).toBe('1')
  })
  it('honors finalizer readiness and installation conditions', () => {
    const { result, invoked } = runFinalizer('', { MUNIMENT_E2E_FINALIZER_TEST_READY: '0', MUNIMENT_E2E_FINALIZER_TEST_INSTALLED: '0' })
    expect(result.status).toBe(0)
    expect(invoked).not.toContain('revoke-session')
    expect(invoked).not.toContain('remove-package')
    expect(invoked).toContain('package-gone')
  })
  it.each(phases)('executes the real finalizer after an injected %s failure', (failed) => {
    const { result, invoked } = runFinalizer(failed)
    expect(result.status).not.toBe(0)
    expect(invoked).toContain(failed)
    const failureIndex = invoked.indexOf(failed)
    const applicableLater = phases.slice(phases.indexOf(failed) + 1).filter((phase) => {
      if (failed === 'redact-artifacts') return !['replace-artifacts', 'publish-artifacts'].includes(phase)
      if (failed === 'replace-artifacts') return phase !== 'publish-artifacts'
      if (failed === 'suppress-artifacts') return true
      return phase !== 'suppress-artifacts'
    })
    for (const phase of applicableLater) expect(invoked.indexOf(phase)).toBeGreaterThan(failureIndex)
    if (['redact-artifacts', 'replace-artifacts', 'publish-artifacts'].includes(failed)) expect(invoked).toContain('suppress-artifacts')
  })
})
