import { afterEach, describe, expect, it } from 'vitest'
import { execFileSync, spawn, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { startWebDriver } from '@wdio/utils'

const root = process.cwd()
const temporary = []
const temp = () => { const value = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-e2e-test-')); temporary.push(value); return value }
afterEach(() => { for (const value of temporary.splice(0)) fs.rmSync(value, { recursive: true, force: true }) })
const runNode = (script, args, options = {}) => spawnSync(process.execPath, [path.join(root, script), ...args], { encoding: 'utf8', ...options })

describe('installed onboarding spec contract', () => {
  const onboardingSpec = fs.readFileSync(path.join(root, 'test/e2e/specs/onboarding.spec.js'), 'utf8')

  it.each(['onboarding.spec.js', 'real-sign-in.spec.js'])('%s uses only shipped onboarding controls', (name) => {
    const spec = fs.readFileSync(path.join(root, 'test/e2e/specs', name), 'utf8')
    const selectors = [...spec.matchAll(/data-testid=(["'])(onboarding-[^"']+)\1/g)].map((match) => match[2])
    expect(selectors.length).toBeGreaterThan(0)
    expect(new Set(selectors)).toEqual(new Set(['onboarding-home-path', 'onboarding-picker', 'onboarding-confirm']))
  })

  it('bounds the first render wait and names its diagnostic log', () => {
    expect(onboardingSpec).toMatch(/location\.waitForDisplayed\(\{\s*timeout: 120000,/)
    expect(onboardingSpec).toContain("timeoutMsg: 'model-ready onboarding first render did not show the Home picker'")
    expect(onboardingSpec).toContain("'onboarding-first-render.log'")
  })
})

describe('installed production chat contract', () => {
  const spec = fs.readFileSync(path.join(root, 'test/e2e/specs/real-sign-in.spec.js'), 'utf8')

  it('submits a unique image prompt and verifies its rendered attachment and assistant token', () => {
    expect(spec).toMatch(/const prompt = `Muniment E2E image check \$\{Date\.now\(\)\}/)
    expect(spec).toContain("const expectedToken = 'MUNIMENT-PLUM-4827'")
    expect(spec.match(/const prompt = ([^\n]+)/)?.[1]).not.toContain('expectedToken')
    expect(spec).toContain("await dialog.mockReturnValue(attachmentPath)")
    expect(spec).toContain("$('button=Add files')")
    expect(spec).toContain('submittedAttachment.waitForDisplayed()')
    expect(spec).toContain("const send = await $('button=Send')")
    expect(spec).toContain('await send.click()')
    expect(spec).toContain('userMessage.waitForDisplayed()')
    expect(spec).toContain("expect(assistantText).not.toBe('')")
    expect(spec).toContain('expect(assistantText).toContain(expectedToken)')
  })

  it('keeps the image fixture in isolated runner state rather than diagnostics', () => {
    const linux = fs.readFileSync(path.join(root, 'test/e2e/runner/linux.sh'), 'utf8')
    const windows = fs.readFileSync(path.join(root, 'test/e2e/runner/windows.ps1'), 'utf8')
    const fixture = Buffer.from(fs.readFileSync(path.join(root, 'test/e2e/fixtures/image-token.png.base64'), 'utf8'), 'base64')
    expect(fixture.subarray(0, 8).toString('hex')).toBe('89504e470d0a1a0a')
    expect(linux).toContain('image_fixture="$state_root/image-token.png"')
    expect(linux).toContain('MUNIMENT_E2E_IMAGE_PATH="$image_fixture"')
    expect(windows).toContain('$imageFixture = Join-Path $stateRoot "image-token.png"')
    expect(windows).toContain('$env:MUNIMENT_E2E_IMAGE_PATH = $imageFixture')
    expect(linux).not.toContain('$raw/image-token.png')
    expect(windows).not.toContain('Join-Path $raw "image-token.png"')
  })

  it('uses a bounded completion condition and verifies the server receipt route', () => {
    expect(spec).toContain('this.timeout(360000)')
    expect(spec).toContain('await browser.waitUntil(async () => {')
    expect(spec).toContain('timeout: 180000')
    expect(spec).toContain('chat response did not complete with a receipt for prompt:')
    expect(spec).not.toMatch(/browser\.pause\s*\(/)
    expect(spec).toContain("response.$('button.provenance')")
    expect(spec).toContain("response.$('.route-value')")
    expect(spec).toContain("route.getText()).trim()).not.toBe('')")
  })

  it('does not capture the rendered production conversation', () => {
    expect(spec.slice(spec.indexOf('const prompt ='))).not.toContain('saveScreenshot')
  })
})

describe('WDIO Tauri driver contract', () => {
  it('loads the installed ESM entry with compatible transitive named exports', async () => {
    await expect(import('@wdio/tauri-service')).resolves.toBeDefined()
  }, 15_000)

  it('keeps WebdriverIO in remote mode at the external Tauri driver endpoint', async () => {
    const previousBinary = process.env.MUNIMENT_E2E_APP_BINARY
    const previousArtifacts = process.env.MUNIMENT_E2E_RAW_DIR
    const previousExternalDriver = process.env.MUNIMENT_E2E_EXTERNAL_DRIVER
    process.env.MUNIMENT_E2E_APP_BINARY = path.join(root, 'muniment-test-binary')
    process.env.MUNIMENT_E2E_RAW_DIR = temp()
    process.env.MUNIMENT_E2E_EXTERNAL_DRIVER = '1'
    try {
      const { config } = await import('./e2e/wdio.conf.js?endpoint-contract')
      expect(config.capabilities).toEqual([{
        'tauri:options': { application: process.env.MUNIMENT_E2E_APP_BINARY },
      }])
      expect(config.hostname).toBe('127.0.0.1')
      expect(config.port).toBe(4444)
      await expect(startWebDriver(config)).resolves.toBeUndefined()
      expect(config.services).toEqual([])
    } finally {
      if (previousBinary === undefined) delete process.env.MUNIMENT_E2E_APP_BINARY
      else process.env.MUNIMENT_E2E_APP_BINARY = previousBinary
      if (previousArtifacts === undefined) delete process.env.MUNIMENT_E2E_RAW_DIR
      else process.env.MUNIMENT_E2E_RAW_DIR = previousArtifacts
      if (previousExternalDriver === undefined) delete process.env.MUNIMENT_E2E_EXTERNAL_DRIVER
      else process.env.MUNIMENT_E2E_EXTERNAL_DRIVER = previousExternalDriver
    }
  })

  it('starts and waits for the external Tauri driver around every Linux WDIO run', () => {
    const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/linux.sh'), 'utf8')
    const runE2e = runner.slice(runner.indexOf('run_e2e()'), runner.indexOf('\nemit_artifacts()'))
    expect(runE2e).toContain('tauri-driver --port 4444 >"$driver_log" 2>&1 &')
    expect(runE2e).toContain('/dev/tcp/127.0.0.1/4444')
    expect(runE2e.indexOf('/dev/tcp/127.0.0.1/4444')).toBeLessThan(runE2e.indexOf('npm run test:e2e'))
    expect(runE2e).toContain("stop_matching '[t]auri-driver'")
    expect(runner).toContain('export MUNIMENT_E2E_EXTERNAL_DRIVER=1')
    expect(runner.match(/run_e2e .*driver-(?:onboarding|app|cleanup)\.log/g)).toHaveLength(3)
  })
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
    expect(runner).toContain('window_wait_seconds=120')
    expect(runner).toContain('window_deadline=$((SECONDS + window_wait_seconds))')
    expect(runner).toContain('>"$raw/first-window-timeout.log"')
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

  it('defaults the artifact directory to the path desktop-ci actually collects', () => {
    expect(runner).toContain('$artifacts = if ($env:DCI_ARTIFACTS_DIR) { $env:DCI_ARTIFACTS_DIR } else { Join-Path $env:TEMP "dci-artifacts" }')
    expect(runner).not.toContain('C:\\dci-artifacts')
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
    ['rendered production conversation', { '03-chat-complete.png': Buffer.from('not safe') }, {}],
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
  // The driver's own --collect-artifacts fence, used when the guest never
  // published an envelope of its own (runner contract).
  const driverMarkers = (body) => `-----DESKTOP-CI-ARTIFACTS-BEGIN-----\n${body}\n-----DESKTOP-CI-ARTIFACTS-END-----\n`
  const archive = (setup) => {
    const source = temp(); setup(source)
    return execFileSync('tar', ['-czf', '-', '-C', source, '.']).toString('base64')
  }
  const logArchive = (name, body = 'safe') => archive((dir) => fs.writeFileSync(path.join(dir, name), body))
  const extractInto = (output, runStatus) => {
    const file = path.join(temp(), 'output'); const destination = path.join(temp(), 'artifacts'); fs.writeFileSync(file, output)
    const args = [path.join(root, 'test/e2e/support/extract-artifacts.sh'), file, destination]
    if (runStatus !== undefined) args.push(String(runStatus))
    const result = spawnSync('bash', args, { encoding: 'utf8' })
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

  it('falls back to the driver envelope when the guest published none', () => {
    const { result, destination } = extractInto(`[desktop-ci 01:02:03] collecting artifacts from VM\n${driverMarkers(logArchive('junit-results.xml', '<testsuites/>'))}`)
    expect(result.status, result.stderr).toBe(0)
    expect(fs.readFileSync(path.join(destination, 'junit-results.xml'), 'utf8')).toBe('<testsuites/>')
  })

  it('accepts the wrapped payload and surviving certutil fences the driver really emits', () => {
    const wrapped = logArchive('wdio.log').match(/.{1,76}/g).join('\n')
    const { result, destination } = extractInto(driverMarkers(`-----BEGIN CERTIFICATE-----\n${wrapped}\n-----END CERTIFICATE-----`))
    expect(result.status, result.stderr).toBe(0)
    expect(fs.readFileSync(path.join(destination, 'wdio.log'), 'utf8')).toBe('safe')
  })

  it('prefers the redacted guest envelope when both forms are present', () => {
    const { result, destination } = extractInto(`${markers(logArchive('guest.log'))}${driverMarkers(logArchive('driver.log'))}`)
    expect(result.status, result.stderr).toBe(0)
    expect(fs.existsSync(path.join(destination, 'guest.log'))).toBe(true)
    expect(fs.existsSync(path.join(destination, 'driver.log'))).toBe(false)
  })

  it('reports a malformed guest envelope rather than falling through to the driver form', () => {
    const { result, destination } = extractInto(`${markers('junk')}${markers('junk')}${driverMarkers(logArchive('driver.log'))}`)
    expect(result.status).not.toBe(0)
    expect(diagnostics(destination).stage).toBe('markers')
    expect(fs.existsSync(path.join(destination, 'driver.log'))).toBe(false)
  })

  it.each([
    ['duplicated', (body) => `${driverMarkers(body)}${driverMarkers(body)}`],
    ['interleaved', (body) => `-----DESKTOP-CI-ARTIFACTS-BEGIN-----\n-----DESKTOP-CI-ARTIFACTS-BEGIN-----\n${body}\n-----DESKTOP-CI-ARTIFACTS-END-----\n`],
    ['unterminated', (body) => `-----DESKTOP-CI-ARTIFACTS-BEGIN-----\n${body}\n`],
    ['reversed', (body) => `-----DESKTOP-CI-ARTIFACTS-END-----\n${body}\n-----DESKTOP-CI-ARTIFACTS-BEGIN-----\n`],
  ])('rejects %s driver markers', (_name, wrap) => {
    expect(extract(wrap(logArchive('driver.log'))).status).not.toBe(0)
  })

  it('rejects a driver-form archive with a link member', () => {
    expect(extract(driverMarkers(archive((dir) => fs.symlinkSync('/tmp', path.join(dir, 'link'))))).status).not.toBe(0)
  })

  it('rejects a driver-form archive with a traversal member', () => {
    const source = temp(); fs.writeFileSync(path.join(source, 'file'), 'unsafe')
    const encoded = execFileSync('tar', ['-czf', '-', '--transform=s,^,../,', '-C', source, 'file']).toString('base64')
    expect(extract(driverMarkers(encoded)).status).not.toBe(0)
  })

  it('rejects a corrupt driver-form payload', () => {
    const { result, destination } = extractInto(driverMarkers('not*valid*base64'))
    expect(result.status).not.toBe(0)
    expect(diagnostics(destination).stage).toBe('base64')
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

  it('records the desktop-ci exit status so a driver fault is not read as a bad envelope', () => {
    expect(diagnostics(extractInto('no envelope\n', 3).destination).desktop_ci_exit_status).toBe('3')
    expect(diagnostics(extractInto('no envelope\n', 0).destination).desktop_ci_exit_status).toBe('0')
  })

  it('records an absent or non-numeric exit status as unrecorded', () => {
    expect(diagnostics(extractInto('no envelope\n').destination).desktop_ci_exit_status).toBe('unrecorded')
    expect(diagnostics(extractInto('no envelope\n', '1 corp-secret').destination).desktop_ci_exit_status).toBe('unrecorded')
  })

  it('names the failing desktop-ci stage from the driver\'s own fixed constants', () => {
    const transcript = [
      '[desktop-ci 01:02:03] waiting for desktop-CI slot (lock)...',
      '[desktop-ci 01:02:04] slot acquired',
      '[desktop-ci 01:07:04] FATAL: SSH not reachable after 300s',
      '[desktop-ci 01:07:05] BUILD FAILED (linux) rc=3',
      '[desktop-ci 01:07:06] WARN: no artifacts collected (VM had no artifact dir or SSH failed)',
    ].join('\n')
    const { result, destination } = extractInto(`${transcript}\n`, 3)
    expect(result.status).not.toBe(0)
    const fields = diagnostics(destination)
    expect(fields.desktop_ci_exit_status).toBe('3')
    expect(fields.driver_log_line_count).toBe('5')
    expect(fields.driver_fatal_ssh_unreachable).toBe('1')
    expect(fields.driver_build_failed).toBe('1')
    expect(fields.driver_no_artifacts_warning).toBe('1')
    expect(fields.driver_fatal_no_slot).toBe('0')
    expect(fields.driver_fatal_clone_failed).toBe('0')
    expect(fields.driver_build_green).toBe('0')
    expect(fields.driver_artifact_marker_count).toBe('0')
    expect(fields.driver_screendump_marker_count).toBe('0')
  })

  it('counts the driver artifact and screendump markers it could not parse', () => {
    const { destination } = extractInto([
      '-----DESKTOP-CI-ARTIFACTS-BEGIN-----',
      '-----DESKTOP-CI-ARTIFACTS-BEGIN-----',
      'not*valid*base64',
      '-----DESKTOP-CI-ARTIFACTS-END-----',
      '-----DESKTOP-CI-SCREENDUMP-BEGIN-----',
      'cGF5bG9hZA==',
      '-----DESKTOP-CI-SCREENDUMP-END-----',
    ].join('\n') + '\n')
    const fields = diagnostics(destination)
    expect(fields.stage).toBe('markers')
    expect(fields.driver_artifact_marker_count).toBe('2')
    expect(fields.driver_screendump_marker_count).toBe('1')
  })

  it('never leaks transcript content into the diagnostics', () => {
    const { destination } = extractInto('=== DESKTOP-CI ARTIFACTS BEGIN ===\ncorp-secret-value AKIA0123456789 Bearer sk-live-abcdefg\n[desktop-ci 01:02:03] BUILD FAILED corp-secret-value\n')
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
    const dir = temp(); const ledger = path.join(dir, 'ledger'); const statusLedger = path.join(dir, 'status-ledger'); const artifacts = path.join(dir, 'artifacts')
    fs.mkdirSync(artifacts)
    const junit = extraEnv.MUNIMENT_E2E_FINALIZER_TEST_STATUS === '1'
      ? '<testsuite name="installed-linux" failures="1"><testcase><failure message="safe diagnostic"/></testcase></testsuite>'
      : '<testsuite name="installed-linux" failures="0"/>'
    fs.writeFileSync(path.join(artifacts, 'junit-results.xml'), junit)
    const result = spawnSync('bash', [path.join(root, 'test/e2e/runner/linux.sh')], {
      encoding: 'utf8',
      env: { ...process.env, DCI_ARTIFACTS_DIR: artifacts, MUNIMENT_E2E_FINALIZER_TEST_MODE: '1', MUNIMENT_E2E_FINALIZER_TEST_LEDGER: ledger, MUNIMENT_E2E_FINALIZER_TEST_STATUS_LEDGER: statusLedger, MUNIMENT_E2E_FINALIZER_TEST_FAIL: failed, ...extraEnv },
    })
    const entries = fs.readFileSync(ledger, 'utf8').trim().split('\n')
    const statuses = Object.fromEntries(fs.readFileSync(statusLedger, 'utf8').trim().split('\n').map((entry) => entry.split('\t')))
    return { result, entries, statuses, invoked: entries.map((entry) => entry.split('\t')[0]) }
  }
  const commands = (entries) => Object.fromEntries(entries.map((entry) => entry.split('\t', 2)))
  const envelopeMarkers = (stdout) => [
    stdout.match(/^=== DESKTOP-CI ARTIFACTS BEGIN ===$/gm)?.length ?? 0,
    stdout.match(/^=== DESKTOP-CI ARTIFACTS END ===$/gm)?.length ?? 0,
  ]
  const extractEnvelope = (stdout) => {
    const output = path.join(temp(), 'output'); const extracted = path.join(temp(), 'extracted')
    fs.writeFileSync(output, stdout)
    const result = spawnSync('bash', [path.join(root, 'test/e2e/support/extract-artifacts.sh'), output, extracted], { encoding: 'utf8' })
    return { result, extracted }
  }
  it('prefers the packaged binary name and retains the legacy fallback', () => {
    expect(runner).toContain('app_binary=$(command -v muniment-desktop || command -v muniment)')
  })
  it.each([
    ['successful run', {}, 0],
    ['product-test failure', { MUNIMENT_E2E_FINALIZER_TEST_STATUS: '1' }, 1],
  ])('emits one extractable safe envelope after a %s', (_name, env, expectedStatus) => {
    const { result } = runFinalizer('', env)
    expect(result.status).toBe(expectedStatus)
    expect(envelopeMarkers(result.stdout)).toEqual([1, 1])
    const { result: extraction, extracted } = extractEnvelope(result.stdout)
    expect(extraction.status, extraction.stderr).toBe(0)
    const junit = fs.readFileSync(path.join(extracted, 'junit-results.xml'), 'utf8')
    expect(junit).toContain('installed-linux')
    if (expectedStatus) expect(junit).toContain('<failure message="safe diagnostic"/>')
  })
  // A failed cleanup step used to swallow the whole bundle, so the failures most
  // in need of evidence -- an early guest abort -- reported nothing but an
  // opaque "desktop-ci infrastructure" result. Redaction stays the only gate.
  it.each(['stop-app', 'remove-raw', 'remove-cleanup-log'])(
    'still publishes the redacted bundle after injected %s cleanup failure', (failed) => {
      const { result } = runFinalizer(failed, { MUNIMENT_E2E_FINALIZER_TEST_STATUS: '1' })
      expect(result.status).not.toBe(0)
      expect(envelopeMarkers(result.stdout)).toEqual([1, 1])
      const { result: extraction, extracted } = extractEnvelope(result.stdout)
      expect(extraction.status, extraction.stderr).toBe(0)
      expect(fs.readFileSync(path.join(extracted, 'junit-results.xml'), 'utf8')).toContain('<failure message="safe diagnostic"/>')
      const fallback = spawnSync('bash', [path.join(root, 'test/e2e/support/ensure-junit-report.sh'), extracted, 'installed-linux', '1', '0'], { encoding: 'utf8' })
      expect(fallback.status, fallback.stderr).toBe(0)
      expect(fs.existsSync(path.join(extracted, 'junit-infrastructure.xml'))).toBe(false)
    },
  )
  it.each([
    ['redact-artifacts', 'redaction-failed'],
    ['replace-artifacts', 'publication-failed'],
    ['publish-artifacts', 'publication-failed'],
  ])('publishes only fixed cleanup labels after injected %s failure', (failed, reason) => {
    const { result } = runFinalizer(failed, { MUNIMENT_E2E_FINALIZER_TEST_STATUS: '1' })
    expect(result.status).not.toBe(0)
    expect(envelopeMarkers(result.stdout)).toEqual([1, 1])
    const { result: extraction, extracted } = extractEnvelope(result.stdout)
    expect(extraction.status, extraction.stderr).toBe(0)
    expect(fs.readdirSync(extracted).sort()).toEqual(['cleanup-status.log', 'envelope-reason.txt'])
    expect(fs.readFileSync(path.join(extracted, 'envelope-reason.txt'), 'utf8')).toContain(`reason: ${reason}`)
    const ledger = fs.readFileSync(path.join(extracted, 'cleanup-status.log'), 'utf8').trim().split('\n')
    expect(ledger).toContain(`${failed}: failed`)
    for (const line of ledger) expect(line).toMatch(/^[a-z-]+: (?:ok|failed)$/)
  })
  it('suppresses the envelope when the publication channel itself fails', () => {
    const { result } = runFinalizer('publish-envelope')
    expect(result.status).not.toBe(0)
    expect(result.stdout).not.toContain('=== DESKTOP-CI ARTIFACTS')
  })
  it('clears stale automation before reaching recovery, then tears down the app', () => {
    const { result, entries, invoked } = runFinalizer()
    const command = commands(entries)
    expect(result.status).toBe(0)
    expect(invoked.slice(0, 5)).toEqual(['stop-wdio', 'stop-driver', 'revoke-session', 'stop-browser-driver', 'stop-app'])
    expect(command['stop-wdio']).toBe("stop_matching \\[w\\]dio.\\\*test/e2e/wdio.conf.js ")
    expect(command['stop-driver']).toBe("stop_matching \\[t\\]auri-driver ")
    expect(command['revoke-session']).toBe('run_cleanup_e2e ')
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
    expect(command['replace-artifacts']).toMatch(/^rm -rf -- \/tmp\/muniment-e2e-test-[^/]+\/artifacts $/)
    expect(command['publish-artifacts']).toBe(`mv -- ${safe} ${command['replace-artifacts'].slice('rm -rf -- '.length)}`)
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
