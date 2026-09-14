import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { execFileSync, spawn, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import './e2e/support/windows-msi-registration-contract.js'

const root = process.cwd()
const temporary = []
const temp = () => { const value = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-e2e-test-'))); temporary.push(value); return value }
afterEach(() => { for (const value of temporary.splice(0)) fs.rmSync(value, { recursive: true, force: true }) })
const runNode = (script, args, options = {}) => spawnSync(process.execPath, [path.join(root, script), ...args], { encoding: 'utf8', ...options })

describe('installed onboarding spec contract', () => {
  const onboardingSpec = fs.readFileSync(path.join(root, 'test/e2e/specs/onboarding.spec.js'), 'utf8')

  it.each([
    ['onboarding.spec.js', ['onboarding-home-path', 'onboarding-model', 'onboarding-scan', 'onboarding-picker', 'onboarding-confirm']],
    ...['real-sign-in.spec.js', 'local-mode-chat.spec.js'].map((name) => [name, ['onboarding-home-path']]),
  ])('%s checks the one-screen onboarding controls', (name, expectedSelectors) => {
    const spec = fs.readFileSync(path.join(root, 'test/e2e/specs', name), 'utf8')
    const selectors = [...spec.matchAll(/data-testid=(["'])(onboarding-[^"']+)\1/g)].map((match) => match[2])
    expect(selectors.length).toBeGreaterThan(0)
    expect(new Set(selectors)).toEqual(new Set(expectedSelectors))
  })

  it('bounds the first render wait and names its diagnostic log', () => {
    expect(onboardingSpec).toMatch(/location\.waitForDisplayed\(\{\s*timeout: 120000,/)
    expect(onboardingSpec).toContain("timeoutMsg: 'model-ready onboarding first render did not show the Home chip'")
    expect(onboardingSpec).toContain("'onboarding-first-render.log'")
  })

  it('drives the Home dialog without a production mock transport', () => {
    expect(onboardingSpec).not.toContain('browser.tauri.mock')
    expect(onboardingSpec).toContain('chooseFolder(')
  })

  it('reports the desktop client when the model settings wait runs out', () => {
    expect(onboardingSpec).toContain("timeoutMsg: 'model settings did not appear after the first Send'")
    expect(onboardingSpec).toContain('throw new Error(`${waitError.message} Model settings: ${failure} ${await shellState()}`)')
    expect(onboardingSpec).toContain('return `desktop client status: ${connection}. shell: ${rendered}`')
    expect(onboardingSpec.indexOf('async function shellState()')).toBeLessThan(onboardingSpec.indexOf("describe('installed nightly model-ready onboarding'"))
  })
})

describe('installed production chat contract', () => {
  const spec = fs.readFileSync(path.join(root, 'test/e2e/specs/real-sign-in.spec.js'), 'utf8')

  it('submits a unique prompt and verifies a nonempty assistant response', () => {
    expect(spec).toMatch(/const prompt = `Muniment E2E chat \$\{Date\.now\(\)\}`/)
    expect(spec).toContain('const send = await $(\'button[aria-label="Send"]\')')
    expect(spec).toContain('await send.click()')
    expect(spec).toContain('userMessage.waitForDisplayed()')
    expect(spec).toContain("expect(assistantText).not.toBe('')")
    expect(spec).not.toContain('browser.tauri.mock')
    expect(spec).not.toContain("plugin:dialog|open")
    expect(spec).toContain("const home = await location.getProperty('textContent')")
  })

  it('waits for the desktop client to connect before it submits the prompt', () => {
    // Anchor on the connection gate itself. The diagnostic helper reads the same
    // command earlier in the file, so the first invoke is no longer the gate.
    const connectionWait = spec.indexOf('attachStatus.supervisor_running === true && attachStatus.connected === true')
    const prompt = spec.indexOf('const prompt = `Muniment E2E chat')
    expect(connectionWait).toBeGreaterThan(-1)
    expect(connectionWait).toBeLessThan(prompt)
    expect(spec).toContain("window.__TAURI__.core.invoke('attach_listener_status')")
  })

  it('bounds the signed-out wait and reports the desktop client in its message', () => {
    expect(spec).toMatch(/browser\.waitUntil\(async \(\) => \(\s*await signedOut\.isDisplayed\(\) \|\| await localMode\.isDisplayed\(\)\s*\), \{\s*timeout: 120000,/)
    expect(spec).toContain("timeoutMsg: 'the signed-out screen and Local mode did not appear after onboarding'")
    expect(spec).toContain('throw new Error(`${waitError.message} ${await shellState()}`)')
    expect(spec).toContain('return `desktop client status: ${connection}. shell: ${rendered}`')
    expect(spec.indexOf('async function shellState()')).toBeLessThan(spec.indexOf("describe('installed nightly'"))
  })

  it('runs and stops the installed runtime inside each Linux E2E session', () => {
    const linux = fs.readFileSync(path.join(root, 'test/e2e/runner/linux.sh'), 'utf8')
    expect(linux).toContain('/usr/lib/muniment/muniment-runtime >>"$2" 2>&1 &')
    expect(linux).toContain('kill "$runtime_pid" 2>/dev/null || true')
    expect(linux).toContain('if ! kill -0 "$runtime_pid" 2>/dev/null; then')
    expect(linux).toContain('pkill -"$signal" -f \'^/usr/lib/muniment/muniment-runtime( |$)\'')
    expect(linux).toContain("! pgrep -f '^/usr/lib/muniment/muniment-runtime( |$)'")
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
    expect(spec).toContain('}, rawDir)).timeout(900000)')
    expect(spec).toContain('await browser.waitUntil(async () => {')
    expect(spec).toContain('timeout: 180000')
    expect(spec).toContain('The chat response returned neither a receipt nor a refusal for prompt:')
    expect(spec).not.toMatch(/browser\.pause\s*\(/)
    expect(spec).toContain("response.$('button.provenance')")
    expect(spec).toContain("response.$('.route-value')")
    expect(spec).toContain("route.getText()).trim()).not.toBe('')")
  })

  it('does not capture the rendered production conversation', () => {
    expect(spec.slice(spec.indexOf('const prompt ='))).not.toContain('saveScreenshot')
  })

  it('drives hosted native authorization through Continue and Approve', () => {
    expect(spec).toContain('main[data-native-authorization="pending"]')
    expect(spec).toContain('button[name="action"][value="login"]')
    expect(spec).toContain('button[name="action"][value="select"]')
    expect(spec).toContain('button[name="action"][value="approve"]')
    expect(spec).toContain('window.location.assign(target)')
    expect(spec).toContain("timeoutMsg: 'production authorization did not ask for approval'")
    expect(spec).toContain("timeoutMsg: 'production sign-in did not return to the desktop callback'")
  })
})

describe('WDIO main window selection', () => {
  beforeEach(() => {
    vi.resetModules()
    vi.stubEnv('MUNIMENT_E2E_APP_BINARY', path.join(root, 'muniment-test-binary'))
    vi.stubEnv('MUNIMENT_E2E_RAW_DIR', temp())
  })
  afterEach(() => {
    vi.unstubAllEnvs()
    vi.unstubAllGlobals()
  })

  function driverFixture(batches, labels) {
    let current
    let attempt = 0
    const calls = []
    const driver = {
      waitUntil: async (predicate, options) => {
        expect(options.timeout).toBe(30000)
        for (attempt = 0; attempt < 3; attempt++) {
          if (await predicate()) return
        }
        throw new Error(options.timeoutMsg)
      },
      tauri: { switchWindow: vi.fn(async (label) => {
        current = Object.keys(labels).find((handle) => labels[handle] === label)
      }) },
      getWindowHandles: async () => batches[Math.min(attempt, batches.length - 1)],
      switchToWindow: async (handle) => {
        calls.push(handle)
        if (labels[handle] instanceof Error) throw labels[handle]
        current = handle
      },
      execute: async (script) => {
        vi.stubGlobal('window', { __TAURI__: { window: { getCurrentWindow: () => ({ label: labels[current] }) } } })
        return script()
      },
    }
    return { driver, calls, label: () => labels[current] }
  }

  it.each([
    [['launcher', 'main'], { launcher: 'launcher', main: 'main' }],
    [['main', 'launcher'], { launcher: 'launcher', main: 'main' }],
    [['opaque-a', 'opaque-b'], { 'opaque-a': 'launcher', 'opaque-b': 'main' }],
    [['opaque-b', 'opaque-a'], { 'opaque-a': 'launcher', 'opaque-b': 'main' }],
    [['main', 'shell'], { main: 'launcher', shell: 'main' }],
  ])('selects the Tauri main label before specs and tests with handles %j', async (handles, labels) => {
    const { config } = await import('./e2e/wdio.conf.js')
    const fixture = driverFixture([handles], labels)
    vi.stubGlobal('browser', fixture.driver)
    await config.before({}, ['test/e2e/specs/onboarding.spec.js'])
    expect(fixture.label()).toBe('main')
    await fixture.driver.switchToWindow(handles.find((handle) => labels[handle] === 'launcher'))
    await config.beforeTest()
    expect(fixture.driver.tauri.switchWindow).toHaveBeenCalledWith('main')
    expect(fixture.label()).toBe('main')
  })

  it('waits for the main window and tolerates a stale handle', async () => {
    const { selectMainWindow } = await import('./e2e/wdio.conf.js')
    const fixture = driverFixture([[], ['stale', 'launcher'], ['stale', 'shell']], {
      stale: new Error('window closed'), launcher: 'launcher', shell: 'main',
    })
    await selectMainWindow(fixture.driver)
    expect(fixture.calls).toEqual(['stale', 'launcher', 'stale', 'shell'])
    expect(fixture.label()).toBe('main')
  })

  it.each([[[]], [['launcher']], [['stale']]])('names the handles when main stays absent from %j', async (handles) => {
    const { selectMainWindow } = await import('./e2e/wdio.conf.js')
    const { driver } = driverFixture([handles], { launcher: 'launcher', stale: new Error('window closed') })
    await expect(selectMainWindow(driver)).rejects.toThrow(`The driver could not select the main window. Handles: ${JSON.stringify(handles)}`)
  })

  it('captures only main after a failure leaves the launcher current', async () => {
    const { config, captureFailureArtifacts } = await import('./e2e/wdio.conf.js')
    const fixture = driverFixture([['launcher', 'main']], { launcher: 'launcher', main: 'main' })
    vi.stubGlobal('browser', fixture.driver)
    await config.before({}, ['test/e2e/specs/onboarding.spec.js'])
    await fixture.driver.switchToWindow('launcher')
    fixture.driver.getPageSource = async () => `<${fixture.label()}>shell</${fixture.label()}>`
    fixture.driver.saveScreenshot = async () => { expect(fixture.label()).toBe('main') }
    await captureFailureArtifacts({ passed: false })
    expect(fs.readFileSync(path.join(process.env.MUNIMENT_E2E_RAW_DIR, 'page-source-onboarding.html'), 'utf8')).toBe('<main>shell</main>')
  })

  it('The harness captures a timeout when Mocha bypasses afterTest.', async () => {
    const { config } = await import('./e2e/wdio.conf.js')
    const fixture = driverFixture([['main']], { main: 'main' })
    vi.stubGlobal('browser', fixture.driver)
    await config.before({}, ['test/e2e/specs/onboarding.spec.js'])
    fixture.driver.getPageSource = vi.fn(async () => '<main><section class="onboarding">The runtime is not connected yet. Open model settings again.</section></main>')
    fixture.driver.saveScreenshot = vi.fn(async () => {})
    await config.after(1)
    expect(fs.readFileSync(path.join(process.env.MUNIMENT_E2E_RAW_DIR, 'page-source-onboarding.html'), 'utf8'))
      .toContain('The runtime is not connected yet.')
    expect(fixture.driver.saveScreenshot).toHaveBeenCalled()
    fixture.driver.getPageSource.mockClear()
    await config.after(0)
    expect(fixture.driver.getPageSource).not.toHaveBeenCalled()
  })

  it('does not save launcher artifacts when main selection fails', async () => {
    const { captureFailureArtifacts } = await import('./e2e/wdio.conf.js')
    const capture = {
      selectMainWindow: vi.fn().mockRejectedValue(new Error('main missing')),
      getPageSource: vi.fn(), saveScreenshot: vi.fn(), writeFile: vi.fn(), log: vi.fn(),
    }
    await captureFailureArtifacts({ passed: false }, capture)
    expect(capture.log).toHaveBeenCalledWith('Failed to select the main window for failure capture.', expect.any(Error))
    expect(capture.getPageSource).not.toHaveBeenCalled()
    expect(capture.saveScreenshot).not.toHaveBeenCalled()
  })
})

describe('WDIO Tauri driver contract', () => {
  it('keeps production capabilities for both windows in the e2e build', () => {
    const e2e = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri/tauri.e2e.conf.json'), 'utf8'))
    const capabilities = e2e.app.security.capabilities
    expect(capabilities).toContain('default')
    expect(capabilities).toContain('launcher')
    const launcher = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri/capabilities/launcher.json'), 'utf8'))
    expect(launcher.windows).toEqual(['launcher'])
    expect(launcher.permissions).toContain('core:event:default')
    expect(capabilities.find((entry) => entry.identifier === 'e2e-webdriver').windows).toEqual(['main', 'launcher'])
  })

  it.each([
    ['linux.sh', /-name 'page-source-\*\.html'[\s\S]+-name 'screenshot-\*\.png'/],
    ['macos.sh', /-name 'page-source-\*\.html'[\s\S]+-name 'screenshot-\*\.png'/],
    ['windows.ps1', /-like "page-source-\*\.html"[\s\S]+-like "screenshot-\*\.png"/],
  ])('%s collects failure page source and screenshots', (name, pattern) => {
    expect(fs.readFileSync(path.join(root, 'test/e2e/runner', name), 'utf8')).toMatch(pattern)
  })

  it('redacts every fixture value from captured page source', async () => {
    const previousBinary = process.env.MUNIMENT_E2E_APP_BINARY
    const previousArtifacts = process.env.MUNIMENT_E2E_RAW_DIR
    const previousUsername = process.env.MUNIMENT_E2E_USERNAME
    const previousPassword = process.env.MUNIMENT_E2E_PASSWORD
    process.env.MUNIMENT_E2E_APP_BINARY = path.join(root, 'muniment-test-binary')
    process.env.MUNIMENT_E2E_RAW_DIR = temp()
    process.env.MUNIMENT_E2E_USERNAME = 'user@example.test'
    process.env.MUNIMENT_E2E_PASSWORD = 'secret'
    try {
      const { redactPageSource } = await import('./e2e/wdio.conf.js?redaction-contract')
      expect(redactPageSource('user@example.test secret user@example.test secret'))
        .toBe('[REDACTED] [REDACTED] [REDACTED] [REDACTED]')
    } finally {
      if (previousBinary === undefined) delete process.env.MUNIMENT_E2E_APP_BINARY
      else process.env.MUNIMENT_E2E_APP_BINARY = previousBinary
      if (previousArtifacts === undefined) delete process.env.MUNIMENT_E2E_RAW_DIR
      else process.env.MUNIMENT_E2E_RAW_DIR = previousArtifacts
      if (previousUsername === undefined) delete process.env.MUNIMENT_E2E_USERNAME
      else process.env.MUNIMENT_E2E_USERNAME = previousUsername
      if (previousPassword === undefined) delete process.env.MUNIMENT_E2E_PASSWORD
      else process.env.MUNIMENT_E2E_PASSWORD = previousPassword
    }
  })

  it('does not propagate capture failures and attempts both captures', async () => {
    const previousBinary = process.env.MUNIMENT_E2E_APP_BINARY
    const previousArtifacts = process.env.MUNIMENT_E2E_RAW_DIR
    process.env.MUNIMENT_E2E_APP_BINARY = path.join(root, 'muniment-test-binary')
    process.env.MUNIMENT_E2E_RAW_DIR = temp()
    try {
      const { captureFailureArtifacts } = await import('./e2e/wdio.conf.js?capture-contract')
      const calls = []
      await expect(captureFailureArtifacts({ passed: false }, {
        selectMainWindow: async () => { calls.push('main') },
        getPageSource: async () => { calls.push('source'); throw new Error('source failed') },
        saveScreenshot: async () => { calls.push('screenshot'); throw new Error('screenshot failed') },
        writeFile: async () => { calls.push('write') },
        log: () => { throw new Error('log failed') },
      })).resolves.toBeUndefined()
      expect(calls).toEqual(['main', 'source', 'screenshot'])
      await captureFailureArtifacts({ passed: true }, {
        selectMainWindow: async () => { calls.push('passing main') },
        getPageSource: async () => { calls.push('passing source') },
        saveScreenshot: async () => { calls.push('passing screenshot') },
        writeFile: async () => { calls.push('passing write') },
        log: () => {},
      })
      expect(calls).toEqual(['main', 'source', 'screenshot'])
    } finally {
      if (previousBinary === undefined) delete process.env.MUNIMENT_E2E_APP_BINARY
      else process.env.MUNIMENT_E2E_APP_BINARY = previousBinary
      if (previousArtifacts === undefined) delete process.env.MUNIMENT_E2E_RAW_DIR
      else process.env.MUNIMENT_E2E_RAW_DIR = previousArtifacts
    }
  }, 15_000)

  it.skipIf(process.platform === 'win32')('Keeps distinct captures after two failed spec runs.', () => {
    const directory = temp()
    const capture = path.join(directory, 'capture.mjs')
    fs.writeFileSync(capture, `
import fs from 'node:fs/promises'
import { config } from ${JSON.stringify(pathToFileURL(path.join(root, 'test/e2e/wdio.conf.js')).href)}
const spec = process.argv[2]
globalThis.browser = {
  waitUntil: async (predicate) => { if (!await predicate()) throw new Error('main missing') },
  getWindowHandles: async () => ['launcher', 'main'],
  switchToWindow: async (handle) => { globalThis.window = { __TAURI__: { window: { getCurrentWindow: () => ({ label: handle }) } } } },
  execute: async (script) => script(),
  getPageSource: async () => '<main>' + spec + ' failed after Send</main>',
  saveScreenshot: async (destination) => fs.copyFile(process.argv[3], destination),
}
await config.before({}, [spec])
await config.afterTest({}, {}, { passed: false })
`)
    const screenshot = path.join(directory, 'fixture.png')
    fs.writeFileSync(screenshot, Buffer.from(fs.readFileSync(path.join(root, 'test/e2e/fixtures/image-token.png.base64'), 'utf8'), 'base64'))
    for (const spec of ['local-mode-chat', 'real-sign-in']) {
      const result = spawnSync(process.execPath, [capture, `test/e2e/specs/${spec}.spec.js`, screenshot], {
        encoding: 'utf8', timeout: 10_000,
        env: { ...process.env, MUNIMENT_E2E_APP_BINARY: path.join(directory, 'app'), MUNIMENT_E2E_RAW_DIR: directory },
      })
      expect(result.status, result.stderr).toBe(0)
    }
    for (const spec of ['local-mode-chat', 'real-sign-in']) {
      expect(fs.readFileSync(path.join(directory, `page-source-${spec}.html`), 'utf8'))
        .toBe(`<main>test/e2e/specs/${spec}.spec.js failed after Send</main>`)
      expect(fs.readFileSync(path.join(directory, `screenshot-${spec}.png`))).toEqual(fs.readFileSync(screenshot))
    }
    expect(fs.existsSync(path.join(directory, 'page-source-installed.html'))).toBe(false)
    const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/linux.sh'), 'utf8')
    const index = runner.slice(runner.indexOf('index_failure_artifacts()'), runner.indexOf('\ncollect_local_mode_pi_log()'))
    const result = spawnSync('bash', ['-c', `raw=$1\n${index}\nindex_failure_artifacts`, 'bash', directory], { encoding: 'utf8' })
    expect(result.status, result.stderr).toBe(0)
    expect(fs.readFileSync(path.join(directory, 'failure-artifacts.log'), 'utf8').trim().split('\n').map((file) => path.basename(file)).sort())
      .toEqual(['page-source-local-mode-chat.html', 'page-source-real-sign-in.html', 'screenshot-local-mode-chat.png', 'screenshot-real-sign-in.png'])
  })

  it('loads the installed ESM entry with compatible transitive named exports', async () => {
    await expect(import('@wdio/tauri-service')).resolves.toBeDefined()
  }, 15_000)

  it('uses the embedded WebDriver provider', async () => {
    const previousBinary = process.env.MUNIMENT_E2E_APP_BINARY
    const previousArtifacts = process.env.MUNIMENT_E2E_RAW_DIR
    process.env.MUNIMENT_E2E_APP_BINARY = path.join(root, 'muniment-test-binary')
    process.env.MUNIMENT_E2E_RAW_DIR = temp()
    try {
      const { config } = await import('./e2e/wdio.conf.js?endpoint-contract')
      expect(config.capabilities).toEqual([{ browserName: 'tauri' }])
      expect(config.services[0][1]).toMatchObject({
        appBinaryPath: process.env.MUNIMENT_E2E_APP_BINARY,
        driverProvider: 'embedded',
      })
    } finally {
      if (previousBinary === undefined) delete process.env.MUNIMENT_E2E_APP_BINARY
      else process.env.MUNIMENT_E2E_APP_BINARY = previousBinary
      if (previousArtifacts === undefined) delete process.env.MUNIMENT_E2E_RAW_DIR
      else process.env.MUNIMENT_E2E_RAW_DIR = previousArtifacts
    }
  })

  it('runs every Linux phase without an external Tauri driver', () => {
    const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/linux.sh'), 'utf8')
    const runE2e = runner.slice(runner.indexOf('run_e2e()'), runner.indexOf('\nemit_artifacts()'))
    expect(runE2e).toMatch(/dbus-run-session[\s\S]+xdg-desktop-portal[\s\S]+npm run test:e2e/)
    expect(runner).not.toMatch(/tauri-driver|4444|MUNIMENT_E2E_EXTERNAL_DRIVER/)
    expect(runner.match(/run_e2e "\$raw\/wdio-(?:onboarding|cleanup|sign-in)\.log"|run_e2e "\$raw\/wdio\.log"/g)).toHaveLength(4)
  })
})

describe.skipIf(process.platform === 'win32')('Linux spec process isolation', () => {
  const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/linux.sh'), 'utf8')
  const functions = runner.slice(runner.indexOf('cleanup_absent()'), runner.indexOf('\nemit_artifacts()'))
  const sequence = runner.slice(runner.indexOf('# Run the installed chat specs first.'))
  const processes = ['app', 'desktop', 'runtime', 'driver', 'wdio']
  const runSequence = (failedSpec = 'local-mode-chat', stopMode = 'delayed') => {
    const directory = temp()
    const script = path.join(directory, 'sequence.sh')
    fs.writeFileSync(script, `set -uo pipefail
raw="$1"
state_root="$raw/state"
process_root="$raw/processes"
cleanup_log="$raw/cleanup.log"
cleanup_status_ledger="$raw/cleanup-status.log"
cleanup_status=0
status=0
trap 'exit "$status"' EXIT
mkdir -p "$process_root"
source test/e2e/support/cleanup-ledger.sh
${functions}
process_key() {
  case "$*" in
    *muniment-runtime*) key=runtime ;;
    *muniment-desktop*) key=desktop ;;
    *WebKitWebDriver*) key=driver ;;
    *wdio*) key=wdio ;;
    *muniment*) key=app ;;
    *) exit 99 ;;
  esac
}
pgrep() { local key; process_key "$@"; [[ -e "$process_root/$key" ]]; }
pkill() {
  local key; process_key "$@"
  printf 'signal: %s %s\\n' "$1" "$key" >>"$cleanup_log"
  if [[ $STOP_MODE == immediate || ( $STOP_MODE == force && $1 == -KILL ) ]]; then
    rm -f "$process_root/$key"
  fi
}
sleep() {
  printf 'wait\\n' >>"$cleanup_log"
  if [[ $STOP_MODE == delayed ]]; then rm -f "$process_root/"*; fi
}
dbus-run-session() {
  local spec=onboarding
  case "$*" in
    *local-mode-chat.spec.js*) spec=local-mode-chat ;;
    *real-sign-in.spec.js*) spec=real-sign-in ;;
  esac
  if [[ -n $(ls -A "$process_root") ]]; then
    printf 'stale-processes: %s\\n' "$spec" >>"$cleanup_log"
    return 91
  fi
  printf 'session: %s\\n' "$spec" >>"$cleanup_log"
  touch "$raw/xdg-desktop-portal.log" "$raw/driver-app.log"
  printf 'muniment-runtime: run_id=fixture-%s pi_stderr_tail=["provider failed"]\\n' "$spec" >>"$raw/muniment-runtime.log"
  mkdir -p "$MUNIMENT_STATE_DIR/sessions"
  printf '{"message":{"provider":"ollama","stopReason":"error","errorMessage":"%s provider failed"}}\\n' "$spec" >"$MUNIMENT_STATE_DIR/sessions/session.jsonl"
  for key in app desktop runtime driver wdio; do touch "$process_root/$key"; done
  [[ $spec != "$FAILED_SPEC" ]]
}
${sequence}
`)
    const result = spawnSync('bash', [script, directory], {
      encoding: 'utf8', timeout: 10_000,
      env: { ...process.env, FAILED_SPEC: failedSpec, STOP_MODE: stopMode, MUNIMENT_E2E_FINALIZER_TEST_MODE: '0' },
    })
    return { result, directory, log: fs.readFileSync(path.join(directory, 'cleanup.log'), 'utf8') }
  }

  it('Keeps the local mode Pi log before sign-in replaces the session log.', () => {
    const { result, directory, log } = runSequence('local-mode-chat')
    expect(result.status, result.stderr).toBe(1)
    const piLog = fs.readFileSync(path.join(directory, 'pi-local-mode-chat.log'), 'utf8')
    expect(piLog).toContain('local-mode-chat provider failed')
    expect(piLog).not.toContain('real-sign-in provider failed')
    expect(piLog).not.toContain('onboarding provider failed')
    expect(log).toContain('The runner saved pi-local-mode-chat.log.')
    expect(log.indexOf('The runner saved')).toBeLessThan(log.indexOf('session: real-sign-in'))
    const safe = path.join(directory, 'safe')
    expect(runNode('test/e2e/support/redact.mjs', [directory, safe]).status).toBe(0)
    expect(fs.readFileSync(path.join(safe, 'pi-local-mode-chat.log'), 'utf8')).toBe(piLog)
    const stderrLog = fs.readFileSync(path.join(safe, 'pi-local-mode-stderr.log'), 'utf8')
    expect(stderrLog).toContain('run_id=fixture-local-mode-chat pi_stderr_tail=["provider failed"]')
    expect(stderrLog).not.toContain('real-sign-in')
    expect(stderrLog).not.toContain('onboarding')
    const emitter = runner.slice(runner.indexOf('emit_artifacts()'), runner.indexOf('\nemit_minimal_artifacts()'))
    const envelope = spawnSync('bash', ['-c', `${emitter}\nemit_artifacts "$1"`, 'bash', safe], { encoding: 'utf8' })
    expect(envelope.status, envelope.stderr).toBe(0)
    const archive = Buffer.from(envelope.stdout.split('\n')[1], 'base64')
    const files = execFileSync('tar', ['-tzf', '-'], { input: archive, encoding: 'utf8' })
    expect(files).toContain('./muniment-runtime.log')
    expect(files).toContain('./pi-local-mode-stderr.log')
  })

  it.each(['missing', 'empty', 'multiple', 'copy-failure'])('Collects Pi log evidence with %s session state.', (state) => {
    const directory = temp()
    const data = path.join(directory, 'state')
    const sessions = path.join(data, 'sessions')
    if (state !== 'missing') fs.mkdirSync(sessions, { recursive: true })
    if (state === 'multiple' || state === 'copy-failure') {
      fs.writeFileSync(path.join(sessions, 'first.jsonl'), '{"errorMessage":"provider failed with fixture-secret"}\n')
      fs.writeFileSync(path.join(sessions, 'second.jsonl'), '{"stopReason":"stop"}\n')
      fs.writeFileSync(path.join(sessions, 'auth.json'), 'Do not collect credentials.')
      fs.symlinkSync(path.join(sessions, 'auth.json'), path.join(sessions, 'linked.jsonl'))
    }
    const collector = runner.slice(runner.indexOf('collect_local_mode_pi_log()'), runner.indexOf('\n# shellcheck source=../support/runner-failure.sh'))
    const result = spawnSync('bash', ['-c', `set -uo pipefail
raw=$1
MUNIMENT_STATE_DIR=$2
cleanup_log="$raw/cleanup.log"
${collector}
${state === 'copy-failure' ? 'cat() { return 1; }' : ''}
collect_local_mode_pi_log`, 'bash', directory, data], { encoding: 'utf8' })
    expect(result.status, result.stderr).toBe(state === 'copy-failure' ? 1 : 0)
    if (state === 'copy-failure') return
    const safe = path.join(directory, 'safe')
    const redaction = runNode('test/e2e/support/redact.mjs', [directory, safe], {
      env: { ...process.env, MUNIMENT_E2E_PASSWORD: 'fixture-secret' },
    })
    expect(redaction.status, redaction.stderr).toBe(0)
    const piLog = fs.readFileSync(path.join(safe, 'pi-local-mode-chat.log'), 'utf8')
    expect(fs.readFileSync(path.join(safe, 'pi-local-mode-stderr.log'), 'utf8'))
      .toBe('No runtime log exists for the local mode run.\n')
    if (state === 'multiple') {
      expect(piLog).toContain('provider failed with [REDACTED]')
      expect(piLog).toContain('"stopReason":"stop"')
      expect(piLog).not.toContain('fixture-secret')
      expect(piLog).not.toContain('credentials')
      expect(piLog).not.toContain('linked.jsonl')
    } else {
      expect(piLog).toBe('No Pi session log exists for the local mode run.\n')
    }
  })

  it.each(['', 'local-mode-chat', 'real-sign-in'])('Stops all spec processes before the next spec after failure "%s".', (failedSpec) => {
    const { result, log } = runSequence(failedSpec)
    expect(result.status, result.stderr).toBe(failedSpec ? 1 : 0)
    expect(log).not.toContain('stale-processes')
    expect(log.match(/^session: .+$/gm)).toEqual(['session: local-mode-chat', 'session: real-sign-in', 'session: onboarding'])
    for (const [previous, next] of [['local-mode-chat', 'real-sign-in'], ['real-sign-in', 'onboarding']]) {
      const boundary = log.slice(log.indexOf(`session: ${previous}`), log.indexOf(`session: ${next}`))
      for (const process of processes) expect(boundary).toContain(`signal: -TERM ${process}`)
      expect(boundary).toMatch(/wait\nstop-before-spec: ok\nstart-spec:/)
    }
  })

  it('Uses a forced stop when a process ignores the first signal.', () => {
    const { result, log } = runSequence('real-sign-in', 'force')
    expect(result.status, result.stderr).toBe(1)
    const boundary = log.slice(log.indexOf('session: real-sign-in'), log.indexOf('session: onboarding'))
    for (const process of processes) expect(boundary).toContain(`signal: -KILL ${process}`)
    expect(boundary).toContain('stop-before-spec: ok')
    expect(log).not.toContain('stale-processes')
  })

  it('Does not start another spec when a process remains after the forced stop.', () => {
    const { result, log } = runSequence('local-mode-chat', 'stuck')
    expect(result.status, result.stderr).toBe(1)
    expect(log).toContain('stop-before-spec: failed')
    expect(log).not.toContain('session: real-sign-in')
    expect(log).not.toContain('session: onboarding')
    expect(log).not.toContain('start-spec: wdio-onboarding.log')
    expect(result.stderr).toContain('Spec process cleanup failed.')
  })

  it('Accepts an empty process set and immediate process exit.', () => {
    const { result, log } = runSequence('', 'immediate')
    expect(result.status, result.stderr).toBe(0)
    expect(log).not.toContain('wait\n')
    expect(log.match(/stop-before-spec: ok/g)).toHaveLength(3)
  })
})

describe.skipIf(process.platform === 'win32')('macOS WDIO spec process isolation', () => {
  const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/macos-wdio.sh'), 'utf8')
  const functions = fs.readFileSync('test/e2e/support/macos-spec-config.sh', 'utf8') + '\n' + runner.slice(runner.indexOf('harness_processes_gone()'), runner.indexOf('\nfinalize()'))
  const sequence = runner.slice(runner.indexOf('run_step save-config-directory'))
  const processes = ['app', 'desktop', 'runtime', 'tauri-driver', 'safaridriver', 'wdio', 'worker', 'job']
  const specs = ['local-mode-chat', 'real-sign-in', 'onboarding', 'cleanup']
  const runSequence = (failedSpec = '', stopMode = 'delayed', stuckProcess = '') => {
    const directory = temp()
    const script = path.join(directory, 'sequence.sh')
    fs.writeFileSync(script, `set -uo pipefail
raw="$1" HOME="$1/login home"
state_root="$raw/state"
process_root="$raw/processes"
cleanup_log="$raw/cleanup.log"
runtime_log="$raw/runtime.log"
cleanup_status=0
status=0
first_failed_step=none
trap 'restore_macos_spec_config; exit "$status"' EXIT
mkdir -p "$process_root"
source test/e2e/support/cleanup-ledger.sh
source test/e2e/support/runner-failure.sh
${functions}
process_key() {
  case "$*" in
    *muniment-runtime*) key=runtime ;;
    *muniment-desktop*) key=desktop ;;
    *tauri-driver*) key=tauri-driver ;;
    *safaridriver*) key=safaridriver ;;
    *wdio*) key=wdio ;;
    *muniment*) key=app ;;
    *) exit 99 ;;
  esac
}
pgrep() {
  local key; process_key "$@"
  [[ -e "$process_root/$key" ]] || { [[ $key == wdio && -e "$process_root/worker" ]]; }
}
pkill() {
  local key; process_key "$@"
  printf 'signal: %s %s\\n' "$1" "$key" >>"$cleanup_log"
  if [[ $STOP_MODE == immediate || ( $STOP_MODE == force && $1 == -KILL ) ]]; then
    rm -f "$process_root/$key"
    if [[ $key == wdio ]]; then rm -f "$process_root/worker"; fi
  fi
}
node() {
  # The fixture records the forwarder instead of binding a port, and leaves
  # every other node helper alone.
  if [[ \${1:-} == test/e2e/support/ollama-forward.mjs ]]; then
    printf 'ollama-forward: %s\\n' "$2" >>"$cleanup_log"
    return 0
  fi
  command node "$@"
}
launchctl() { if [[ $1 == setenv ]]; then
    if [[ $2 == BUN_CONFIG_VERBOSE_FETCH ]]; then printf 'verbose-fetch: %s\\n' "$3" >>"$cleanup_log"; return; fi
    [[ $3 == "$HOME/Library/Application Support" ]]; return; fi
  if [[ $1 == unsetenv ]]; then printf 'verbose-fetch: cleared\\n' >>"$cleanup_log"; return; fi
  if [[ $1 == print ]]; then
    if [[ -e "$process_root/job" ]]; then printf 'state = running\\npid = 123\\n'; else return 1; fi
  else
    printf 'signal: %s job\\n' "$2" >>"$cleanup_log"
    if [[ $STOP_MODE == immediate || ( $STOP_MODE == force && $2 == SIGKILL ) ]]; then
      rm -f "$process_root/job"
    fi
  fi
}
sleep() {
  printf 'wait\\n' >>"$cleanup_log"
  if [[ $STOP_MODE == delayed ]]; then
    for key in ${processes.join(' ')}; do
      if [[ -e "$process_root/$key" && ( $key != "$STUCK_PROCESS" || ! -e "$raw/first-spec" ) ]]; then rm -f "$process_root/$key"; fi
    done
  fi
}
npm() {
  local spec
  case "$*" in
    'run test:e2e -- --spec test/e2e/specs/local-mode-chat.spec.js') spec=local-mode-chat ;;
    'run test:e2e -- --spec test/e2e/specs/real-sign-in.spec.js') spec=real-sign-in ;;
    'run test:e2e')
      if [[ \${MUNIMENT_E2E_CLEANUP_ONLY:-0} == 1 ]]; then spec=cleanup
      elif [[ \${MUNIMENT_E2E_ONBOARDING_ONLY:-0} == 1 ]]; then spec=onboarding
      else return 98; fi ;;
    *) return 99 ;;
  esac
  if [[ -n $(ls -A "$process_root") ]]; then
    printf 'stale-processes: %s\\n' "$spec" >>"$cleanup_log"
    return 91
  fi
  printf 'session: %s\\n' "$spec" >>"$cleanup_log"
  touch "$raw/first-spec"
  printf 'muniment-runtime: run_id=fixture-%s pi_stderr_tail=["provider failed"]\\n' "$spec" >>"$runtime_log"
  mkdir -p "$HOME/.muniment/sessions"
  printf '{"message":{"provider":"ollama","stopReason":"error","errorMessage":"%s provider failed"}}\\n' "$spec" >"$HOME/.muniment/sessions/session.jsonl"
  for key in ${processes.join(' ')}; do touch "$process_root/$key"; done
  [[ $spec != "$FAILED_SPEC" ]]
}
# Include processes from the installed-build runner before the first spec.
for key in ${processes.join(' ')}; do touch "$process_root/$key"; done
${sequence}
`)
    const result = spawnSync('bash', [script, directory], {
      encoding: 'utf8', timeout: 10_000,
      env: {
        ...process.env, FAILED_SPEC: failedSpec, STOP_MODE: stopMode, STUCK_PROCESS: stuckProcess, MUNIMENT_E2E_LAUNCHCTL: 'launchctl',
        MUNIMENT_E2E_FINALIZER_TEST_MODE: '0', MUNIMENT_E2E_ONBOARDING_ONLY: '0', MUNIMENT_E2E_CLEANUP_ONLY: '0',
      },
    })
    return { result, directory, log: fs.readFileSync(path.join(directory, 'cleanup.log'), 'utf8') }
  }

  it('Keeps the local mode Pi log before sign-in replaces the session log.', () => {
    const { result, directory, log } = runSequence('local-mode-chat')
    expect(result.status, result.stderr).toBe(1)
    const piLog = fs.readFileSync(path.join(directory, 'pi-local-mode-chat.log'), 'utf8')
    expect(piLog).toContain('local-mode-chat provider failed')
    expect(piLog).not.toContain('real-sign-in provider failed')
    expect(fs.readFileSync(path.join(directory, 'pi-local-mode-stderr.log'), 'utf8'))
      .toContain('run_id=fixture-local-mode-chat pi_stderr_tail=["provider failed"]')
    expect(log).toContain('The runner saved pi-local-mode-chat.log.')
    expect(log.indexOf('The runner saved')).toBeLessThan(log.indexOf('session: real-sign-in'))
  })

  it.each(['', ...specs])('Stops all processes before each spec after failure "%s".', (failedSpec) => {
    const { result, log } = runSequence(failedSpec)
    expect(result.status, result.stderr).toBe(failedSpec ? 1 : 0)
    expect(log).not.toContain('stale-processes')
    expect(log.match(/^session: .+$/gm)).toEqual(specs.map((spec) => `session: ${spec}`))
    let start = 0
    for (const spec of specs) {
      const end = log.indexOf(`session: ${spec}`, start)
      const boundary = log.slice(start, end)
      for (const process of processes.filter((value) => value !== 'worker' && value !== 'job')) {
        expect(boundary).toContain(`signal: -TERM ${process}`)
      }
      expect(boundary).toContain('signal: SIGTERM job')
      expect(boundary).toMatch(/wait\nstop-before-spec: ok\nstart-spec:/)
      start = end
    }
  })

  it.each(['immediate', 'force'])('Waits for process exit with stop mode "%s".', (mode) => {
    const { result, log } = runSequence('real-sign-in', mode)
    expect(result.status, result.stderr).toBe(1)
    expect(log).not.toContain('stale-processes')
    expect(log.match(/stop-before-spec: ok/g)).toHaveLength(4)
    if (mode === 'force') expect(log).toContain('signal: -KILL runtime')
    else expect(log).not.toContain('wait\n')
  })

  it.each(processes)('Blocks later specs when %s remains after a failed spec.', (process) => {
    const { result, log } = runSequence('local-mode-chat', 'delayed', process)
    expect(result.status, result.stderr).toBe(1)
    expect(log).toContain('stop-before-spec: failed')
    expect(log.match(/^session: .+$/gm)).toEqual(['session: local-mode-chat'])
    expect(log).not.toContain('stale-processes')
    expect(result.stderr).toContain('Spec process cleanup failed.')
  })

  it('Checks the same process set during final cleanup.', () => {
    expect(runner).toContain('cleanup_step stop-app stop_app\n  cleanup_step restore-config-directory restore_macos_spec_config\n  if (( cleanup_status != 0 )); then status=1; fi')
    expect(runner.indexOf('cleanup_step stop-app stop_app')).toBeLessThan(runner.indexOf('if node test/e2e/support/redact.mjs'))
  })
})

describe.skipIf(process.platform === 'win32')('macOS WDIO startup diagnostics', () => {
  const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/macos-wdio.sh'), 'utf8')
  const setup = runner.slice(0, runner.indexOf('\ncurrent_step=validate-environment'))
  const sequence = runner.slice(runner.indexOf('run_step save-config-directory'))
  const launcher = path.join(root, 'test/e2e/support/macos-wdio-app.sh')
  const specs = ['local-mode-chat', 'real-sign-in', 'onboarding', 'cleanup']

  const installSequence = runner.slice(runner.indexOf('# The installed smoke removes'), runner.indexOf('\nrun_step image-fixture'))

  const runInstalledApp = (mode = 'pass') => {
    const directory = temp()
    const bundle = path.join(directory, 'release/muniment.app')
    const installedBundle = path.join(directory, 'Applications/muniment.app')
    const resources = path.join(bundle, 'Contents/Resources/asr-runtime')
    fs.mkdirSync(resources, { recursive: true })
    fs.mkdirSync(path.join(bundle, 'Contents/MacOS'), { recursive: true })
    fs.writeFileSync(path.join(bundle, 'Contents/MacOS/muniment-desktop'), '#!/bin/sh\nexit 99\n', { mode: 0o755 })
    const helper = path.join(bundle, 'Contents/Library/LaunchServices/muniment-runtime')
    fs.mkdirSync(path.dirname(helper), { recursive: true })
    fs.writeFileSync(helper, 'Developer ID helper fixture', { mode: 0o755 })
    for (const library of ['libsherpa-onnx-c-api.dylib', 'libonnxruntime.1.24.4.dylib']) {
      if (mode !== `missing-${library}`) fs.writeFileSync(path.join(resources, library), 'installed library')
    }
    const app = path.join(directory, 'source app')
    fs.writeFileSync(app, `#!/bin/bash
# TAURI_WEBDRIVER_PORT
[[ -z \${DYLD_LIBRARY_PATH+x} && -z \${DYLD_FALLBACK_LIBRARY_PATH+x} ]] || exit 90
for library in libsherpa-onnx-c-api.dylib libonnxruntime.1.24.4.dylib; do
  [[ -s "$(dirname "$0")/../Resources/asr-runtime/$library" ]] || exit 91
done
printf 'The installed WebDriver app reached the shell.\\n'
`, { mode: 0o755 })
    if (mode === 'existing-bundle') {
      fs.mkdirSync(installedBundle, { recursive: true })
      fs.writeFileSync(path.join(installedBundle, 'sentinel'), 'Keep this bundle.')
    }
    const artifacts = path.join(directory, 'artifacts')
    const result = spawnSync('bash', ['-c', `${setup}
installed_bundle="$FIXTURE_INSTALLED_BUNDLE"
app_binary="$FIXTURE_APP"
stop_app() { [[ $FIXTURE_MODE != stale-process ]]; }
sleep() { :; }
gh() {
  if [[ $* == *releases/tags/nightly ]]; then
    [[ $FIXTURE_MODE != release-failure ]] || return 7
    if [[ $FIXTURE_MODE == missing-asset ]]; then printf '{"assets":[]}'; return; fi
    printf '{"assets":[{"name":"nightly-%s-macos-muniment.app.zip","id":42}]}\\n' "$MUNIMENT_E2E_SOURCE_SHA"
  else
    [[ $FIXTURE_MODE != download-failure ]] || return 7
    printf 'archive fixture'
  fi
}
ditto() {
  if [[ $1 == -x ]]; then
    mkdir -p "$4"
    cp -R "$FIXTURE_BUNDLE" "$4/muniment.app"
  else
    mkdir -p "$(dirname "$2")"
    if [[ $FIXTURE_MODE == install-failure ]]; then mkdir -p "$2"; return 7; fi
    cp -R "$1" "$2"
  fi
}
install() {
  [[ $FIXTURE_MODE != copy-failure ]] || return 7
  if [[ $FIXTURE_MODE == stale-copy ]]; then return 0; fi
  command install "$@"
}
codesign() {
  printf 'codesign: %s\\n' "$*"
  [[ \${@: -1} == "$installed_bundle" ]] || return 92
  if [[ $1 == --force ]]; then
    [[ $# == 4 && $2 == --sign && $3 == - ]] || return 93
    [[ $FIXTURE_MODE != signing-failure ]] || return 7
    cmp -s "$app_binary" "$installed_desktop" || return 94
    printf '# Ad-hoc signature fixture.\\n' >>"$installed_desktop"
  else
    [[ $# == 4 && $1 == --verify && $2 == --deep && $3 == --strict ]] || return 95
    [[ $FIXTURE_MODE != signature-verification-failure ]] || return 7
    grep -q '^# Ad-hoc signature fixture.$' "$installed_desktop" || return 96
    cmp -s "$FIXTURE_BUNDLE/Contents/Library/LaunchServices/muniment-runtime" "$installed_bundle/Contents/Library/LaunchServices/muniment-runtime" || return 97
  fi
}
${installSequence}
export MUNIMENT_E2E_DRIVER_APP_LOG="$raw/driver-app-local-mode-chat.log"
run_step spec-local-mode-chat "$MUNIMENT_E2E_APP_BINARY"
`], {
      encoding: 'utf8', timeout: 10_000,
      env: {
        ...process.env, HOME: directory, TMPDIR: directory, DCI_ARTIFACTS_DIR: artifacts,
        FIXTURE_MODE: mode, FIXTURE_APP: app, FIXTURE_BUNDLE: bundle, FIXTURE_INSTALLED_BUNDLE: installedBundle,
        GH_TOKEN: 'fixture-token', GITHUB_REPOSITORY: 'fixture/desktop',
        MUNIMENT_E2E_SOURCE_SHA: mode === 'invalid-source' ? '' : 'a'.repeat(40),
        DYLD_LIBRARY_PATH: '/invalid/override', DYLD_FALLBACK_LIBRARY_PATH: '/invalid/fallback',
      },
    })
    return { result, artifacts, installedBundle, bundle, app, reason: fs.readFileSync(path.join(artifacts, 'exit-reason.txt'), 'utf8') }
  }

  it('Runs the source build beside the installed libraries without loader overrides.', () => {
    const { result, artifacts, installedBundle, reason } = runInstalledApp()
    expect(result.status, result.stderr).toBe(0)
    expect(reason).toContain('first_failed_step=none\n')
    expect(fs.readFileSync(path.join(artifacts, 'driver-app-local-mode-chat.log'), 'utf8'))
      .toBe('The installed WebDriver app reached the shell.\n')
    expect(fs.existsSync(installedBundle)).toBe(false)
    expect(runner).toContain('installed_bundle=/Applications/muniment.app')
    expect(runner).toContain('installed_desktop="$installed_bundle/Contents/MacOS/muniment-desktop"')
    expect(runner).toContain('export MUNIMENT_E2E_REAL_APP_BINARY="$installed_desktop"')
    expect(runner).not.toMatch(/export DYLD_(?:LIBRARY_PATH|FALLBACK_LIBRARY_PATH)=/)
  })

  it('Signs only the installed WebDriver bundle and verifies its nested signatures before launch.', () => {
    const { result, artifacts, installedBundle, bundle, app } = runInstalledApp()
    expect(result.status, result.stderr).toBe(0)
    const log = fs.readFileSync(path.join(artifacts, 'installer.log'), 'utf8')
    expect(log.split('\n').filter((line) => line.startsWith('codesign:'))).toEqual([
      `codesign: --force --sign - ${installedBundle}`,
      `codesign: --verify --deep --strict ${installedBundle}`,
    ])
    expect(fs.readFileSync(path.join(bundle, 'Contents/MacOS/muniment-desktop'), 'utf8')).toBe('#!/bin/sh\nexit 99\n')
    expect(fs.readFileSync(path.join(bundle, 'Contents/Library/LaunchServices/muniment-runtime'), 'utf8')).toBe('Developer ID helper fixture')
    expect(fs.readFileSync(app, 'utf8')).not.toContain('Ad-hoc signature fixture')
  })

  it.each([
    ['invalid-source', 'validate-source'],
    ['release-failure', 'fetch-release'],
    ['missing-asset', 'identify-asset'],
    ['download-failure', 'download-bundle'],
    ['stale-process', 'stop-before-install'],
    ['install-failure', 'install-bundle'],
    ['missing-libsherpa-onnx-c-api.dylib', 'validate-libsherpa-onnx-c-api.dylib'],
    ['missing-libonnxruntime.1.24.4.dylib', 'validate-libonnxruntime.1.24.4.dylib'],
    ['copy-failure', 'install-webdriver-app'],
    ['stale-copy', 'verify-webdriver-app'],
    ['signing-failure', 'sign-webdriver-app'],
    ['signature-verification-failure', 'verify-webdriver-signature'],
    ['existing-bundle', 'validate-bundle-absent'],
  ])('Names the failed install check for %s before the app starts.', (mode, step) => {
    const { result, artifacts, installedBundle, reason } = runInstalledApp(mode)
    expect(result.status, result.stderr).toBe(1)
    expect(reason).toContain(`first_failed_step=${step}\n`)
    expect(fs.existsSync(path.join(artifacts, 'driver-app-local-mode-chat.log'))).toBe(false)
    if (mode === 'existing-bundle') {
      expect(fs.readFileSync(path.join(installedBundle, 'sentinel'), 'utf8')).toBe('Keep this bundle.')
    } else expect(fs.existsSync(installedBundle)).toBe(false)
  })

  it('Captures an immediate abort and preserves the PID, signal, arguments, and environment.', () => {
    const directory = temp()
    const app = path.join(directory, 'app with spaces')
    const log = path.join(directory, 'driver-app-local-mode-chat.log')
    fs.writeFileSync(app, `#!/bin/sh
ulimit -c 0
printf 'pid=%s port=%s embedded=%s backtrace=%s home=%s\\n' "$$" "$TAURI_WEBDRIVER_PORT" "$WDIO_EMBEDDED_SERVER" "$RUST_BACKTRACE" "$HOME"
printf '<%s>\\n' "$@"
printf 'thread main panicked: fixture startup panic' >&2
kill -ABRT "$$"
`, { mode: 0o700 })
    const result = spawnSync(launcher, ['two words', '', '--flag'], {
      encoding: 'utf8', cwd: directory, timeout: 10_000,
      env: {
        ...process.env, MUNIMENT_E2E_REAL_APP_BINARY: app, MUNIMENT_E2E_DRIVER_APP_LOG: log,
        TAURI_WEBDRIVER_PORT: '4445', WDIO_EMBEDDED_SERVER: 'true', HOME: directory,
      },
    })
    expect(result.status).toBeNull()
    expect(result.signal).toBe('SIGABRT')
    expect(result.stdout).toBe('')
    expect(result.stderr).toBe('')
    const output = fs.readFileSync(log, 'utf8')
    expect(output).toContain(`pid=${result.pid} port=4445 embedded=true backtrace=1 home=${directory}`)
    expect(output).toContain('<two words>\n<>\n<--flag>\n')
    expect(output).toContain('thread main panicked: fixture startup panic')
  })

  const runEnvelope = (mode = 'pass') => {
    const directory = temp()
    const home = path.join(directory, 'login-home')
    const reports = path.join(home, 'Library/Logs/DiagnosticReports')
    const artifacts = path.join(directory, 'artifacts')
    fs.mkdirSync(reports, { recursive: true })
    const runtimeLogs = path.join(home, 'Library/Logs/Muniment')
    if (mode !== 'missing-runtime-log') {
      fs.mkdirSync(runtimeLogs, { recursive: true })
      fs.writeFileSync(path.join(runtimeLogs, 'runtime.log'), 'event=runtime_service_registration_failed domain="SMAppServiceErrorDomain" code=3 description="fixture-secret"\n')
      fs.writeFileSync(path.join(runtimeLogs, 'runtime-service.log'), 'Runtime service fixture-secret.\n')
    }
    const launchctl = path.join(directory, 'launchctl')
    fs.writeFileSync(launchctl, '#!/bin/sh\nprintf "target=%s\\nstate = running\\npid = 123\\n" "$2"\n', { mode: 0o700 })
    const stale = path.join(reports, 'muniment-desktop-stale.ips')
    fs.writeFileSync(stale, 'Stale report.\n')
    fs.utimesSync(stale, new Date(0), new Date(0))
    fs.writeFileSync(path.join(reports, 'other-app.ips'), 'Unrelated report.\n')
    fs.symlinkSync(stale, path.join(reports, 'muniment-desktop-linked.ips'))
    fs.mkdirSync(path.join(reports, 'muniment-desktop-directory.ips'))
    const app = path.join(directory, 'app')
    fs.writeFileSync(app, '#!/bin/sh\nprintf "fixture panic: %s fixture-secret\\n" "$1" >&2\nexit 7\n', { mode: 0o700 })
    const result = spawnSync('bash', ['-c', `${setup}
stop_app() { stop_calls=$(( \${stop_calls:-0} + 1 )); [[ $FIXTURE_MODE != cleanup-failure && ( $FIXTURE_MODE != final-cleanup-only || $stop_calls != 5 ) ]]; }
sleep() {
  if [[ $FIXTURE_MODE == delayed-report ]]; then
    printf 'Late crash report.\\n' >"$diagnostic_reports/muniment-desktop-late.ips"
  fi
}
npm() {
  printf 'WDIO fixture output.\\n'
  if [[ $FIXTURE_MODE == pass || $FIXTURE_MODE == final-cleanup-only ]]; then test -d "$HOME/Documents"; return $?; fi
  "$MUNIMENT_E2E_APP_BINARY" "$current_step"
}
export MUNIMENT_E2E_APP_BINARY="$FIXTURE_LAUNCHER"
export MUNIMENT_E2E_REAL_APP_BINARY="$FIXTURE_APP"
if [[ $FIXTURE_MODE == missing-reports ]]; then rm -rf "$diagnostic_reports"; fi
if [[ $FIXTURE_MODE == startup-failure ]]; then
  run_step build-app bash -c 'echo "Build failed with fixture-secret." >&2; exit 7'
  exit
fi
if [[ $FIXTURE_MODE == signal ]]; then
  current_step=spec-local-mode-chat
  kill -TERM "$$"
fi
if [[ $FIXTURE_MODE == unexpected-exit ]]; then
  current_step=webdriver-marker
  exit 9
fi
${sequence}
if [[ -d $diagnostic_reports ]]; then
  printf 'Crash report with fixture-secret.\\n' >"$diagnostic_reports/muniment-desktop-current.ips"
fi
mkdir -p "$HOME/Library/Logs/Muniment"
printf 'Wrong home.\\n' >"$HOME/Library/Logs/Muniment/runtime.log"
mkdir -p "$HOME/Library/Logs/DiagnosticReports"
printf 'Isolated home crash report.\\n' >"$HOME/Library/Logs/DiagnosticReports/muniment-desktop-isolated.ips"
if [[ $FIXTURE_MODE == redaction-failure ]]; then printf bad >"$raw/screenshot-invalid.png"; fi
if [[ $FIXTURE_MODE == collection-failure ]]; then find() { return 1; }; elif [[ $FIXTURE_MODE == copy-failure ]]; then cp() { return 1; }; fi
if [[ $FIXTURE_MODE == publication-failure ]]; then mv() { return 1; }; fi
`], {
      encoding: 'utf8', timeout: 10_000,
      env: {
        ...process.env, HOME: home, TMPDIR: directory, DCI_ARTIFACTS_DIR: artifacts,
        FIXTURE_MODE: mode, FIXTURE_APP: app, FIXTURE_LAUNCHER: launcher,
        MUNIMENT_E2E_LAUNCHCTL: launchctl,
        MUNIMENT_E2E_PASSWORD: 'fixture-secret', MUNIMENT_E2E_FINALIZER_TEST_MODE: '0',
      },
    })
    return { result, artifacts, directory, reason: fs.readFileSync(path.join(artifacts, 'exit-reason.txt'), 'utf8') }
  }

  it('Keeps each failed spec log and redacts current crash reports.', () => {
    const { result, artifacts, directory, reason } = runEnvelope('abort')
    expect(result.status, result.stderr).toBe(1)
    expect(reason).toBe('status=1\ncleanup_status=0\nredaction_status=0\nfirst_failed_step=spec-local-mode-chat\n')
    for (const spec of specs) {
      const file = path.join(artifacts, `driver-app-${spec}.log`)
      expect(fs.readFileSync(file, 'utf8')).toBe(`fixture panic: spec-${spec} [REDACTED]\n`)
      expect(fs.statSync(file).mode & 0o777).toBe(0o600)
    }
    expect(fs.readFileSync(path.join(artifacts, 'muniment-desktop-current.ips'), 'utf8')).toBe('Crash report with [REDACTED].\n')
    expect(fs.readFileSync(path.join(artifacts, 'muniment-desktop-isolated.ips'), 'utf8')).toBe('Isolated home crash report.\n')
    for (const name of ['muniment-desktop-stale.ips', 'muniment-desktop-linked.ips', 'muniment-desktop-directory.ips', 'other-app.ips']) {
      expect(fs.existsSync(path.join(artifacts, name))).toBe(false)
    }
    expect(fs.readdirSync(directory).filter((name) => name.startsWith('muniment-wdio-macos.'))).toEqual([])
  })

  it.each(['pass', 'abort', 'startup-failure', 'signal'])('Collects login home runtime diagnostics after %s.', (mode) => {
    const { artifacts } = runEnvelope(mode)
    expect(fs.readFileSync(path.join(artifacts, 'runtime.log'), 'utf8'))
      .toBe('event=runtime_service_registration_failed domain="SMAppServiceErrorDomain" code=3 description="[REDACTED]"\n')
    expect(fs.readFileSync(path.join(artifacts, 'runtime-service.log'), 'utf8')).toBe('Runtime service [REDACTED].\n')
    expect(fs.readFileSync(path.join(artifacts, 'runtime-launchctl.log'), 'utf8'))
      .toMatch(/target=gui\/\d+\/ai\.muniment\.runtime\nstate = running\npid = 123\n\nlaunchctl_exit_status=0\n/)
    const ledger = fs.readFileSync(path.join(artifacts, 'cleanup-status.log'), 'utf8')
    expect(ledger.indexOf('collect-runtime-diagnostics: ok')).toBeLessThan(ledger.indexOf('stop-app:'))
  })

  it('Names a missing login home runtime log instead of collecting the isolated home log.', () => {
    const { artifacts, reason } = runEnvelope('missing-runtime-log')
    expect(fs.readFileSync(path.join(artifacts, 'runtime.log'), 'utf8')).toBe('No runtime log exists for this user.\n')
    expect(reason).toContain('cleanup_status=0\n')
  })

  it('Prepares Documents for each spec and publishes empty app logs when all specs pass.', () => {
    const { result, artifacts, reason } = runEnvelope()
    expect(result.status, result.stderr).toBe(0)
    expect(reason).toBe('status=0\ncleanup_status=0\nredaction_status=0\nfirst_failed_step=none\n')
    for (const spec of specs) expect(fs.readFileSync(path.join(artifacts, `driver-app-${spec}.log`), 'utf8')).toBe('')
  })

  it.each(['missing-reports', 'delayed-report'])('Handles the %s crash report case.', (mode) => {
    const { result, artifacts, reason } = runEnvelope(mode)
    expect(result.status, result.stderr).toBe(1)
    expect(reason).toContain('cleanup_status=0')
    if (mode === 'delayed-report') {
      expect(fs.readFileSync(path.join(artifacts, 'muniment-desktop-late.ips'), 'utf8')).toBe('Late crash report.\n')
    }
  })

  it.each([
    ['startup-failure', 'build-app'],
    ['signal', 'spec-local-mode-chat'],
    ['unexpected-exit', 'webdriver-marker'],
    ['cleanup-failure', 'stop-before-spec'], ['final-cleanup-only', 'stop-app'],
  ])('Names the first failed step after %s.', (mode, step) => {
    const { result, artifacts, reason } = runEnvelope(mode)
    expect(result.status, result.stderr).toBe(1)
    expect(reason).toContain(`first_failed_step=${step}\n`)
    if (mode === 'startup-failure') {
      expect(fs.readFileSync(path.join(artifacts, 'runner-stderr.log'), 'utf8')).toBe('Build failed with [REDACTED].\n')
    }
  })

  it.each(['redaction-failure', 'collection-failure', 'copy-failure', 'publication-failure'])('Keeps exit diagnostics after %s.', (mode) => {
    const { result, artifacts, reason } = runEnvelope(mode)
    expect(result.status, result.stderr).toBe(1)
    expect(reason).toContain('cleanup_status=1\n')
    expect(reason).toContain('first_failed_step=spec-local-mode-chat\n')
    expect(reason).toContain(`redaction_status=${mode === 'redaction-failure' ? 1 : 0}\n`)
    expect(fs.readFileSync(path.join(artifacts, 'cleanup-status.log'), 'utf8')).toContain(': failed')
    if (mode === 'redaction-failure') {
      expect(fs.existsSync(path.join(artifacts, 'driver-app-local-mode-chat.log'))).toBe(false)
      expect(fs.readFileSync(path.join(artifacts, 'envelope-reason.txt'), 'utf8')).toContain('reason: redaction-failed')
    }
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
  it.each(['linux', 'macos', 'windows'])('names the expected %s asset and the release assets', (platform) => {
    const name = platform === 'windows' ? `nightly-${sha}-windows-muniment_1.2.3_x64_en-US.msi`
      : platform === 'macos' ? `nightly-${sha}-macos-muniment.app.zip` : asset.name
    const expected = platform === 'windows'
      ? `^nightly-${sha}-windows-muniment_[0-9]+\\.[0-9]+\\.[0-9]+_x64_en-US\\.msi$` : name
    for (const count of [0, 2]) {
      const assets = [{ name: 'unrelated.zip', id: 1 }, ...Array.from({ length: count }, () => ({ name, id: 42 }))]
      const result = validate({ assets }, sha, platform)
      expect(result.status).not.toBe(0)
      expect(result.stderr).toContain(`missing or duplicate ${platform} artifact: expected ${expected}, matches=${count}, release assets=${JSON.stringify(assets.map((entry) => entry.name))}`)
    }
    expect(validate({ assets: [] }, sha, platform).stderr).toContain('matches=0, release assets=[]')
  })
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

describe('Windows auth URL capture seam', { timeout: 30_000 }, () => { // A PowerShell spawn costs about 3.5 seconds, and the slowest observed test took 6993ms.
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

// This block runs a POSIX shell script, and Windows has no shell for it.
describe.skipIf(process.platform === 'win32')('macOS installed launch harness', () => {
  const runnerPath = path.join(root, 'test/e2e/runner/macos.sh')
  const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/macos.sh'), 'utf8')
  const finalizer = runner.slice(runner.indexOf('finalize()'), runner.indexOf('\nif [[ ${MUNIMENT_E2E_FINALIZER_TEST_MODE'))
  const finalizerPhases = [...finalizer.matchAll(/cleanup_step ([a-z-]+)/g)].map((match) => match[1])

  const macosFixture = (failed = '', extraEnv = {}) => {
    const directory = temp(); const ledger = path.join(directory, 'ledger'); const statusLedger = path.join(directory, 'status-ledger'); const artifacts = path.join(directory, 'artifacts')
    const env = { ...process.env, TMPDIR: directory, DCI_ARTIFACTS_DIR: artifacts, MUNIMENT_E2E_RUNTIME_STATE: path.join(directory, 'runtime-state'), MUNIMENT_E2E_FINALIZER_TEST_MODE: '1', MUNIMENT_E2E_FINALIZER_TEST_LEDGER: ledger, MUNIMENT_E2E_FINALIZER_TEST_STATUS_LEDGER: statusLedger, MUNIMENT_E2E_FINALIZER_TEST_FAIL: failed, ...extraEnv }
    const read = (file) => fs.existsSync(file) ? fs.readFileSync(file, 'utf8').trim().split(/\r?\n/).filter(Boolean) : []
    const outcome = (result) => ({ result, artifacts, invoked: read(ledger).map((line) => line.split('\t')[0]), statuses: Object.fromEntries(read(statusLedger).map((line) => line.split('\t'))) })
    return { directory, env, outcome }
  }

  const runMacosFinalizer = (failed = '', extraEnv = {}) => {
    const fixture = macosFixture(failed, extraEnv)
    return fixture.outcome(spawnSync('bash', [runnerPath], { encoding: 'utf8', env: fixture.env }))
  }

  const runMacosEnvelope = (failed = '', extraEnv = {}) => {
    const fixture = macosFixture(failed, { MUNIMENT_E2E_FINALIZER_TEST_EXECUTE: '1', ...extraEnv })
    const setup = runner.slice(0, runner.indexOf('\nif [[ ${MUNIMENT_E2E_FINALIZER_TEST_MODE'))
    const result = spawnSync('bash', ['-c', `${setup}
# The fixture uses real files and replaces only macOS process probes.
stop_app() { :; }
stop_runtime() { :; }
process_absent() { :; }
runtime_process_absent() { :; }
runtime_job_stopped() { :; }
collect_macos_runtime_diagnostics() { :; }
installed_bundle="$expanded/test.app"
installed=1
mkdir -p "$installed_bundle" "$artifacts"
printf 'stale token=%s\\n' "$GH_TOKEN" >"$artifacts/stale.log"
printf 'healthy first window was not visible\\n' >&2
printf 'diagnostic token=%s\\n' "$GH_TOKEN" >&2
if [[ \${FIXTURE_RUNNER_FAILURE:-0} == 1 ]]; then status=1; fi
if [[ \${FIXTURE_LEFTOVER:-0} == 1 ]]; then touch "$run_root/leftover"; fi
if [[ \${FIXTURE_REDACTION_FAILURE:-0} == 1 ]]; then printf '%s' "$GH_TOKEN" >"$raw/screenshot-fixture.png"; fi
if [[ \${FIXTURE_UNEXPECTED_EXIT:-0} == 1 ]]; then exit 7; fi
finalize
`], { encoding: 'utf8', env: fixture.env })
    return { ...fixture.outcome(result), directory: fixture.directory }
  }

  const runMacosPayload = ({ runtime = 'ok', rpath = 'ok', agent = true, field = '', value = '' } = {}) => {
    const directory = temp(); const bundle = path.join(directory, 'muniment.app'); const artifacts = path.join(directory, 'artifacts')
    const runtimePath = path.join(bundle, 'Contents/Library/LaunchServices/muniment-runtime')
    const agentPath = path.join(bundle, 'Contents/Library/LaunchAgents/ai.muniment.runtime.plist')
    const plistBuddy = path.join(directory, 'PlistBuddy')
    const otool = path.join(directory, 'otool')
    fs.mkdirSync(path.dirname(runtimePath), { recursive: true })
    fs.mkdirSync(path.dirname(agentPath), { recursive: true })
    fs.writeFileSync(runtimePath, runtime === 'failure' ? '#!/bin/sh\nexit 1\n' : `#!/bin/sh\nprintf '${runtime === 'empty' ? '   ' : 'muniment-runtime 0.0.1'}\\n'\n`)
    if (runtime !== 'missing') fs.chmodSync(runtimePath, runtime === 'not-executable' ? 0o600 : 0o700)
    else fs.rmSync(runtimePath)
    if (agent) fs.writeFileSync(agentPath, '<plist/>\n')
    fs.writeFileSync(plistBuddy, `#!/bin/sh
key=\${2#Print :}
if [ "$key" = "$MUNIMENT_E2E_TEST_FIELD" ]; then
  [ "$MUNIMENT_E2E_TEST_VALUE" != unavailable ] || exit 1
  printf '%s\\n' "$MUNIMENT_E2E_TEST_VALUE"
  exit
fi
case "$key" in
  Label) printf 'ai.muniment.runtime\\n' ;;
  BundleProgram) printf 'Contents/Library/LaunchServices/muniment-runtime\\n' ;;
  ThrottleInterval) printf '5\\n' ;;
  KeepAlive:SuccessfulExit) printf 'false\\n' ;;
  *) exit 1 ;;
esac
`)
    fs.chmodSync(plistBuddy, 0o700)
    fs.writeFileSync(otool, `#!/bin/sh
[ "$MUNIMENT_E2E_TEST_RPATH" != failure ] || exit 1
[ "$MUNIMENT_E2E_TEST_RPATH" != missing ] || { printf 'Load command 0\\n      cmd LC_LOAD_DYLIB\\n'; exit; }
printf 'Load command 0\\n      cmd LC_RPATH\\n  cmdsize 72\\n     path %s (offset 12)\\n' "$MUNIMENT_E2E_TEST_RPATH"
`)
    fs.chmodSync(otool, 0o700)
    const result = spawnSync('bash', [runnerPath], { encoding: 'utf8', env: {
      ...process.env,
      TMPDIR: directory,
      DCI_ARTIFACTS_DIR: artifacts,
      MUNIMENT_E2E_PAYLOAD_TEST_MODE: '1',
      MUNIMENT_E2E_PAYLOAD_TEST_BUNDLE: bundle,
      MUNIMENT_E2E_PLIST_BUDDY: plistBuddy,
      MUNIMENT_E2E_OTOOL: otool,
      MUNIMENT_E2E_TEST_RPATH: rpath === 'ok' ? '@executable_path/../../Resources/asr-runtime' : rpath,
      MUNIMENT_E2E_TEST_FIELD: field,
      MUNIMENT_E2E_TEST_VALUE: value,
    } })
    return { result, artifacts }
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
    expect(runner).toContain("printf 'last_visible_window_count=%s\\n' \"${window_count:-unavailable}\"")
    expect(runner).toContain('screendump=requested-by-desktop-ci')
  })

  it('waits for the LaunchAgent and the desktop runtime connection', () => {
    const probe = fs.readFileSync(path.join(root, 'test/e2e/support/macos-runtime-probe.sh'), 'utf8')
    const attachService = fs.readFileSync(path.join(root, 'src-tauri/src/attach_service/commands.rs'), 'utf8')
    expect(runner).toContain('probe_macos_runtime "$runtime_target" "$app_pid"')
    expect(runner).toContain('runtime_target="gui/$(id -u)/ai.muniment.runtime"')
    expect(probe).toContain("grep -Eq 'state = running'")
    expect(probe).toContain("grep -Eq 'pid = [1-9][0-9]*'")
    expect(probe).toContain("connection_status == 'desktop runtime client connected=true'")
    expect(probe).toContain('MUNIMENT_E2E_RUNTIME_WAIT_SECONDS:-60')
    expect(probe).toContain('if [[ -S $endpoint ]]')
    expect(attachService).toContain('eprintln!("desktop runtime client connected={connected}")')
  })

  it.each([
    ['inactive job', 'inactive', 'desktop runtime client connected=true\n', 'job_active=false', 'client_connected=true'],
    ['disconnected client', 'active', 'desktop runtime client connected=true\ndesktop runtime client connected=false\n', 'job_active=true', 'client_connected=false'],
  ])('fails the runtime probe for an %s with fixed diagnostics', (_name, job, appOutput, jobResult, clientResult) => {
    const directory = temp(); const launchctl = path.join(directory, 'launchctl'); const appLog = path.join(directory, 'app.log')
    const diagnostic = path.join(directory, 'diagnostic.log'); const endpoint = path.join(directory, 'missing.sock')
    fs.writeFileSync(launchctl, job === 'active' ? '#!/bin/sh\nprintf "state = running\\npid = 42\\n"\n' : '#!/bin/sh\nexit 1\n')
    fs.chmodSync(launchctl, 0o700); fs.writeFileSync(appLog, appOutput)
    const result = spawnSync('bash', ['-c', 'source "$1"; probe_macos_runtime test "$2" "$3" "$4" "$5"', 'bash', path.join(root, 'test/e2e/support/macos-runtime-probe.sh'), String(process.pid), appLog, endpoint, diagnostic], {
      encoding: 'utf8', env: { ...process.env, MUNIMENT_E2E_LAUNCHCTL: launchctl, MUNIMENT_E2E_RUNTIME_WAIT_SECONDS: '1' },
    })
    expect(result.status).not.toBe(0)
    const report = fs.readFileSync(diagnostic, 'utf8')
    expect(report).toContain(jobResult)
    expect(report).toContain(clientResult)
    expect(report).toContain('endpoint_present=false')
    expect(report).not.toContain(directory)
  })

  const collectMacosDiagnostics = ({ job = 'missing', log, serviceLog, secret = '' } = {}) => {
    const directory = temp(); const raw = path.join(directory, 'raw'); const artifacts = path.join(directory, 'safe')
    const launchctl = path.join(directory, 'launchctl'); const runtimeLog = path.join(directory, 'runtime.log')
    fs.mkdirSync(raw)
    fs.writeFileSync(launchctl, job === 'active'
      ? '#!/bin/sh\nprintf "state = running\\npid = 42\\n"\n'
      : '#!/bin/sh\nprintf "Could not find service ai.muniment.runtime\\n" >&2\nexit 113\n', { mode: 0o700 })
    if (log !== undefined) fs.writeFileSync(runtimeLog, log)
    if (serviceLog !== undefined) fs.writeFileSync(path.join(directory, 'runtime-service.log'), serviceLog)
    const result = spawnSync('bash', ['-c', 'source "$1"; collect_macos_runtime_diagnostics gui/501/ai.muniment.runtime "$2" "$3" && node test/e2e/support/redact.mjs "$3" "$4"', 'bash', path.join(root, 'test/e2e/support/macos-runtime-probe.sh'), runtimeLog, raw, artifacts], {
      encoding: 'utf8', env: { ...process.env, MUNIMENT_E2E_LAUNCHCTL: launchctl, GH_TOKEN: secret },
    })
    return { result, artifacts }
  }

  it.each(['active', 'missing'])('publishes native registration errors and launchctl output when the agent is %s', (job) => {
    const reason = 'event=runtime_service_registration_failed domain="SMAppServiceErrorDomain" code=2 description="The plist is invalid."\n'
    const { result, artifacts } = collectMacosDiagnostics({ job, log: reason })
    expect(result.status, result.stderr).toBe(0)
    expect(fs.readFileSync(path.join(artifacts, 'runtime.log'), 'utf8')).toBe(reason)
    const launchctlLog = fs.readFileSync(path.join(artifacts, 'runtime-launchctl.log'), 'utf8')
    expect(launchctlLog).toContain(job === 'active' ? 'state = running\npid = 42' : 'Could not find service ai.muniment.runtime')
    expect(launchctlLog).toContain(`launchctl_exit_status=${job === 'active' ? 0 : 113}`)
    expect(finalizerPhases.indexOf('collect-runtime-diagnostics')).toBeLessThan(finalizerPhases.indexOf('stop-app'))
    expect(fs.statSync(path.join(artifacts, 'runtime.log')).mode & 0o777).toBe(0o600)
  })

  it('names a missing runtime log in the envelope', () => {
    const { result, artifacts } = collectMacosDiagnostics()
    expect(result.status, result.stderr).toBe(0)
    expect(fs.readFileSync(path.join(artifacts, 'runtime.log'), 'utf8')).toBe('No runtime log exists for this user.\n')
    expect(fs.readFileSync(path.join(artifacts, 'runtime-service.log'), 'utf8')).toBe('No runtime service log exists for this user.\n')
  })

  it('The envelope carries runtime service diagnostics beside the desktop activation log.', () => {
    const serviceLog = 'muniment-runtime: started version=test\nmuniment-runtime: run_id=test run_start\n'
    const log = 'event=runtime_service_registered\n'
    const { result, artifacts } = collectMacosDiagnostics({ log, serviceLog })
    expect(result.status, result.stderr).toBe(0)
    expect(fs.readFileSync(path.join(artifacts, 'runtime.log'), 'utf8')).toBe(log)
    expect(fs.readFileSync(path.join(artifacts, 'runtime-service.log'), 'utf8')).toBe(serviceLog)
    expect(fs.statSync(path.join(artifacts, 'runtime-service.log')).mode & 0o777).toBe(0o600)
  })

  it('The envelope keeps an empty runtime service log.', () => {
    const { result, artifacts } = collectMacosDiagnostics({ serviceLog: '' })
    expect(result.status, result.stderr).toBe(0)
    expect(fs.readFileSync(path.join(artifacts, 'runtime-service.log'), 'utf8')).toBe('')
  })

  it('bounds and redacts runtime diagnostics before publication', () => {
    const secret = 'fixture-registration-secret'
    const contents = `${'x'.repeat(300000)}\nAuthorization: Bearer ${secret}\n`
    const { result, artifacts } = collectMacosDiagnostics({ log: contents, serviceLog: contents, secret })
    expect(result.status, result.stderr).toBe(0)
    for (const name of ['runtime.log', 'runtime-service.log']) {
      const log = fs.readFileSync(path.join(artifacts, name), 'utf8')
      expect(Buffer.byteLength(log)).toBeLessThanOrEqual(262144)
      expect(log).not.toContain(secret)
      expect(log).toContain('[REDACTED:credential-header]')
    }
  })

  it('verifies the installed runtime and LaunchAgent without registering the agent', () => {
    const { result, artifacts } = runMacosPayload()
    const payloadLog = fs.existsSync(path.join(artifacts, 'payload.log')) ? fs.readFileSync(path.join(artifacts, 'payload.log'), 'utf8') : ''
    const cleanupLog = fs.existsSync(path.join(artifacts, 'cleanup.log')) ? fs.readFileSync(path.join(artifacts, 'cleanup.log'), 'utf8') : ''
    expect(result.status, `${result.stderr}\n${payloadLog}\n${cleanupLog}`).toBe(0)
    expect(fs.readFileSync(path.join(artifacts, 'payload.log'), 'utf8')).toContain('payload=verified')
    const payloadVerifier = runner.slice(runner.indexOf('verify_installed_payload()'), runner.indexOf('\nstop_app()'))
    expect(payloadVerifier).not.toMatch(/launchctl|SMAppService/)
  })

  it.each([
    ['missing runtime', { runtime: 'missing' }, 'installed runtime is unavailable or not executable'],
    ['non-executable runtime', { runtime: 'not-executable' }, 'installed runtime is unavailable or not executable'],
    ['missing ASR rpath', { rpath: 'missing' }, 'installed runtime ASR rpath is unavailable'],
    ['wrong ASR rpath', { rpath: '@executable_path/../Resources/asr-runtime' }, 'installed runtime ASR rpath is unavailable'],
    ['failed load command probe', { rpath: 'failure' }, 'installed runtime load commands are unavailable'],
    ['failed version probe', { runtime: 'failure' }, 'installed runtime version probe failed'],
    ['empty version', { runtime: 'empty' }, 'installed runtime version is empty'],
    ['missing LaunchAgent', { agent: false }, 'installed runtime LaunchAgent is unavailable'],
    ['missing label', { field: 'Label', value: 'unavailable' }, 'installed runtime LaunchAgent label is unavailable'],
    ['invalid label', { field: 'Label', value: 'wrong' }, 'installed runtime LaunchAgent label is invalid'],
    ['missing executable path', { field: 'BundleProgram', value: 'unavailable' }, 'installed runtime LaunchAgent executable path is unavailable'],
    ['invalid executable path', { field: 'BundleProgram', value: '/tmp/runtime' }, 'installed runtime LaunchAgent executable path is invalid'],
    ['missing throttle', { field: 'ThrottleInterval', value: 'unavailable' }, 'installed runtime LaunchAgent throttle is unavailable'],
    ['invalid throttle', { field: 'ThrottleInterval', value: '0' }, 'installed runtime LaunchAgent throttle is invalid'],
    ['missing unsuccessful-exit policy', { field: 'KeepAlive:SuccessfulExit', value: 'unavailable' }, 'installed runtime LaunchAgent unsuccessful-exit policy is unavailable'],
    ['invalid unsuccessful-exit policy', { field: 'KeepAlive:SuccessfulExit', value: 'true' }, 'installed runtime LaunchAgent unsuccessful-exit policy is invalid'],
  ])('fails for a %s and retains JUnit evidence', (_name, options, message) => {
    const { result, artifacts } = runMacosPayload(options)
    expect(result.status).not.toBe(0)
    expect(fs.readFileSync(path.join(artifacts, 'payload.log'), 'utf8')).toContain(message)
    const junit = spawnSync('bash', [path.join(root, 'test/e2e/support/ensure-junit-report.sh'), artifacts, 'installed-macos', '1', '0'], { encoding: 'utf8' })
    expect(junit.status, junit.stderr).toBe(0)
    expect(fs.readFileSync(path.join(artifacts, 'junit-infrastructure.xml'), 'utf8')).toContain('<failure message=')
  })

  it('probes windows through CoreGraphics, which needs no privacy grant', () => {
    const probe = fs.readFileSync(path.join(root, 'test/e2e/support/macos-window-count.c'), 'utf8')
    expect(runner).not.toMatch(/osascript|System Events/)
    expect(runner).toContain('window_count=$("$window_probe" "$app_pid"')
    expect(probe).toContain('CGWindowListCopyWindowInfo(')
    expect(probe).toContain('kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements')
    // The gate matches the owning pid. A second process with the same name must
    // not satisfy it, and reading a window name would need Screen Recording.
    expect(probe).toContain('read_number(window, kCGWindowOwnerPID, &owner) || owner != pid')
    expect(probe).not.toContain('kCGWindowName')
    expect(probe).not.toContain('CGWindowListCreateImage')
    // The AppleScript probe bounded each call at two seconds; the helper keeps
    // that bound so one read cannot stall the 120-second loop.
    expect(probe).toContain('#define WINDOW_LIST_TIMEOUT_SECONDS 2')
    expect(probe).toContain('alarm(WINDOW_LIST_TIMEOUT_SECONDS)')
  })

  it('builds the window probe before launch and fails the job when it does not compile', () => {
    const build = runner.slice(runner.indexOf('clang '), runner.indexOf('window_ready=0'))
    expect(build).toContain('-framework CoreFoundation -framework CoreGraphics')
    expect(build).toContain('-o "$window_probe" test/e2e/support/macos-window-count.c')
    expect(build).toMatch(/\|\| \{\n\s*echo 'window probe did not compile' >&2; status=1; exit;\n\}/)
    expect(runner.indexOf('clang ')).toBeLessThan(runner.indexOf('app_pid=$!'))
    expect(finalizerPhases).toContain('remove-window-probe')
    expect(finalizerPhases).toContain('window-probe-gone')
  })

  it('contains no sign-in or WebDriver automation', () => {
    expect(runner).not.toMatch(/MUNIMENT_E2E_(?:USERNAME|PASSWORD)/)
    expect(runner).not.toMatch(/wdio|tauri-driver/i)
  })

  it('cleans processes, the installed bundle, and state before redaction and publication', () => {
    const phases = finalizerPhases
    expect(phases.slice(0, 13)).toEqual(['collect-runtime-diagnostics', 'stop-app', 'stop-runtime', 'remove-bundle', 'remove-runtime-state', 'remove-state', 'bundle-gone', 'processes-gone', 'runtime-process-gone', 'runtime-job-stopped', 'runtime-state-gone', 'state-gone', 'stage-cleanup-log'])
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

  it.each(['', 'remove-raw', 'remove-cleanup-log', 'remove-run-root', 'redact-artifacts', 'replace-artifacts', 'publish-artifacts'])(
    'Publishes the full macOS exit diagnostics after %s.', (failed) => {
      const secret = 'macos-stderr-secret'
      const { result, artifacts, directory, invoked, statuses } = runMacosEnvelope(failed, { GH_TOKEN: secret })
      expect(result.status, result.stderr).toBe(failed ? 1 : 0)
      const ledger = fs.readFileSync(path.join(artifacts, 'cleanup-status.log'), 'utf8')
      expect(ledger).toBe(invoked.map((label) => `${label}: ${statuses[label] === '0' ? 'ok' : 'failed'}\n`).join(''))
      expect(ledger).toContain(`remove-run-root: ${statuses['remove-run-root'] === '0' ? 'ok' : 'failed'}\n`)
      expect(invoked.at(-1)).toBe('remove-run-root')
      expect(fs.readFileSync(path.join(artifacts, 'exit-reason.txt'), 'utf8')).toBe(
        `status=0\ncleanup_status=${failed ? 1 : 0}\nredaction_status=${failed === 'redact-artifacts' ? 1 : 0}\nfirst_failed_step=${failed || 'none'}\n`,
      )
      const stderr = fs.readFileSync(path.join(artifacts, 'runner-stderr.log'), 'utf8')
      if (['redact-artifacts', 'replace-artifacts', 'publish-artifacts'].includes(failed)) {
        expect(stderr).toBe('withheld: guest stderr\n')
      } else {
        expect(stderr).toContain('healthy first window was not visible\n')
        expect(stderr).toContain('diagnostic token=')
      }
      expect(fs.existsSync(path.join(artifacts, 'stale.log'))).toBe(false)
      for (const file of fs.readdirSync(artifacts)) {
        expect(fs.readFileSync(path.join(artifacts, file), 'utf8')).not.toContain(secret)
      }
      if (!failed) expect(fs.readdirSync(directory).filter((name) => name.startsWith('muniment-e2e-macos.'))).toEqual([])
    },
  )

  it('Publishes fixed diagnostics when the redactor rejects a screenshot.', () => {
    const secret = 'macos-screenshot-secret'
    const { result, artifacts } = runMacosEnvelope('', { GH_TOKEN: secret, FIXTURE_REDACTION_FAILURE: '1' })
    expect(result.status).toBe(1)
    expect(fs.readFileSync(path.join(artifacts, 'exit-reason.txt'), 'utf8')).toBe(
      'status=0\ncleanup_status=1\nredaction_status=1\nfirst_failed_step=redact-artifacts\n',
    )
    expect(fs.readFileSync(path.join(artifacts, 'cleanup-status.log'), 'utf8')).toContain('remove-run-root: ok\n')
    expect(fs.readFileSync(path.join(artifacts, 'runner-stderr.log'), 'utf8')).toBe('withheld: guest stderr\n')
    expect(fs.readFileSync(path.join(artifacts, 'redaction-failure.txt'), 'utf8')).toContain('category: screenshot-format\n')
    expect(fs.existsSync(path.join(artifacts, 'stale.log'))).toBe(false)
    for (const file of fs.readdirSync(artifacts)) {
      expect(fs.readFileSync(path.join(artifacts, file), 'utf8')).not.toContain(secret)
    }
  })

  it('Names an unexpected runner exit.', () => {
    const { result, artifacts } = runMacosEnvelope('', { GH_TOKEN: 'fixture-secret', FIXTURE_UNEXPECTED_EXIT: '1' })
    expect(result.status).toBe(1)
    expect(fs.readFileSync(path.join(artifacts, 'exit-reason.txt'), 'utf8')).toBe(
      'status=1\ncleanup_status=0\nredaction_status=0\nfirst_failed_step=runner\n',
    )
  })

  it('Names a real run-root removal failure.', () => {
    const { result, artifacts } = runMacosEnvelope('', { GH_TOKEN: 'fixture-secret', FIXTURE_LEFTOVER: '1' })
    expect(result.status).toBe(1)
    expect(fs.readFileSync(path.join(artifacts, 'cleanup-status.log'), 'utf8')).toContain('remove-run-root: failed\n')
    expect(fs.readFileSync(path.join(artifacts, 'exit-reason.txt'), 'utf8')).toContain('first_failed_step=remove-run-root\n')
  })

  it('Keeps the runner failure ahead of a cleanup failure.', () => {
    const { result, artifacts } = runMacosEnvelope('remove-raw', { GH_TOKEN: 'fixture-secret', FIXTURE_RUNNER_FAILURE: '1' })
    expect(result.status).toBe(1)
    expect(fs.readFileSync(path.join(artifacts, 'exit-reason.txt'), 'utf8')).toBe(
      'status=1\ncleanup_status=1\nredaction_status=0\nfirst_failed_step=runner\n',
    )
  })

  it('publishes a minimal report without the planted secret after redaction fails', () => {
    const plantedSecret = 'macos-planted-secret'
    const { result, artifacts } = runMacosFinalizer('redact-artifacts', {
      MUNIMENT_E2E_PASSWORD: plantedSecret,
    })
    expect(result.status).not.toBe(0)
    expect(fs.readdirSync(artifacts).sort()).toEqual(['cleanup-status.log', 'envelope-reason.txt', 'exit-reason.txt', 'redaction-failure.txt', 'runner-stderr.log'])
    expect(fs.readFileSync(path.join(artifacts, 'envelope-reason.txt'), 'utf8')).toContain('reason: redaction-failed')
    const report = fs.readFileSync(path.join(artifacts, 'redaction-failure.txt'), 'utf8')
    expect(report).toBe('file: unknown\ncategory: redactor-process\n')
    expect(report).not.toContain(plantedSecret)
  })
})

describe('Windows spec process isolation', () => {
  const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/windows.ps1'), 'utf8')
  const functions = runner.slice(runner.indexOf('function Get-HarnessProcesses'), runner.indexOf('function Remove-AuthHandler'))

  it('Runs each spec through the process cleanup gate.', () => {
    expect(runner).toContain("Invoke-E2e (Join-Path $raw \"wdio.log\") \"Windows local-mode tests failed\" 'test/e2e/specs/local-mode-chat.spec.js'")
    expect(runner).toContain("Invoke-E2e (Join-Path $raw \"wdio-sign-in.log\") \"Windows sign-in tests failed\" 'test/e2e/specs/real-sign-in.spec.js'")
    expect(runner).toContain('Invoke-E2e (Join-Path $raw "wdio-onboarding.log") "Windows onboarding tests failed"')
    expect(runner).toMatch(/Invoke-Cleanup "revoke-session" \{\s+Stop-HarnessProcesses[\s\S]+Invoke-BoundedProcess "npm.cmd" "run test:e2e" 45/)
    expect(functions).toContain('Get-Process muniment, muniment-desktop, muniment-runtime, msedgedriver')
    expect(functions).toContain("'*@wdio*local-runner*run.js*'")
    expect(functions).toContain('Stop-ScheduledTask -InputObject $task')
    expect(functions).toContain("$processes.Count -eq 0 -and (-not $task -or $task.State -ne 'Running')")
  })

  it.skipIf(process.platform !== 'win32').each([false, true])('Stops the failed spec before the next spec with stop failure %s.', (stopFails) => {
    const directory = temp()
    const script = path.join(directory, 'sequence.ps1')
    fs.writeFileSync(script, `param([string]$Directory, [string]$StopFails)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$raw = $Directory
$cleanupLog = Join-Path $Directory 'cleanup.log'
$cleanupStatusLedger = Join-Path $Directory 'cleanup-status.log'
$cleanupStatus = 0
$script:active = $false
$script:starts = 0
${functions}
function Stop-HarnessProcesses {
  if ($script:active -and $StopFails -eq 'true') { throw 'Fixture process did not stop.' }
  $script:active = $false
}
function Invoke-NativeCommand {
  if ($script:active) { throw 'Fixture found a stale process.' }
  $script:active = $true
  $script:starts++
  Add-Content $cleanupLog "fixture-start: $script:starts"
  if ($script:starts -eq 1) { throw 'Fixture spec failed.' }
}
try { Invoke-E2e (Join-Path $raw 'first.log') 'First spec failed.' } catch { Add-Content $cleanupLog $_.Exception.Message }
try { Invoke-E2e (Join-Path $raw 'second.log') 'Second spec failed.' } catch { Add-Content $cleanupLog $_.Exception.Message }
`)
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script, directory, String(stopFails)], {
      encoding: 'utf8', timeout: 20_000,
      env: { ...process.env, MUNIMENT_E2E_FINALIZER_TEST_MODE: '0', MUNIMENT_E2E_FINALIZER_TEST_FAIL: '' },
    })
    expect(result.status, result.stderr).toBe(0)
    const log = fs.readFileSync(path.join(directory, 'cleanup.log'), 'utf8').replaceAll('\r\n', '\n')
    expect(log).toContain('Fixture spec failed.')
    expect(log).not.toContain('Fixture found a stale process.')
    if (stopFails) {
      expect(log).toContain('stop-before-spec: failed')
      expect(log).not.toContain('fixture-start: 2')
    } else {
      expect(log).toContain('stop-before-spec: ok\nstart-spec: second.log\nfixture-start: 2')
    }
  }, 30_000)
})

describe('Windows MSI identity contract', { timeout: 30_000 }, () => {
  const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/windows.ps1'), 'utf8')
  const start = runner.indexOf('  $installer = New-Object -ComObject WindowsInstaller.Installer')
  const identity = runner.slice(start, runner.indexOf('\n  $env:APPDATA', start))

  it('Passes typed COM arguments and checks the product name before installation.', () => {
    expect(identity).toContain('@([string]$msi, [int]0)')
    expect(identity).toContain('@([string]"SELECT')
    expect(identity).toContain('@([int]1)')
    expect(identity).toContain('if ($null -eq $record) { throw "package ProductName is missing" }')
    expect(identity).toContain('if ($productName -cne "muniment") { throw "package identity mismatch" }')
    expect(identity).toContain('Write-Output "MSI ProductName: $productName"')
    expect(start).toBeLessThan(runner.lastIndexOf('\n  Install-Product'))
  })

  it.skipIf(process.platform !== 'win32').each([
    ['muniment', 0, 'MSI ProductName: muniment'],
    ['another product', 1, 'package identity mismatch'],
    ['Muniment', 1, 'package identity mismatch'],
    ['', 1, 'package ProductName is missing'],
  ])('Reads an MSI fixture with ProductName "%s".', (name, status, message) => {
    const directory = temp()
    const script = path.join(directory, 'identity.ps1')
    fs.writeFileSync(script, `param([string]$DatabasePath, [string]$FixtureName)
$ErrorActionPreference = "Stop"
$seed = New-Object -ComObject WindowsInstaller.Installer
$db = $seed.GetType().InvokeMember("OpenDatabase", "InvokeMethod", $null, $seed, @([string]$DatabasePath, [int]3))
$sql = 'CREATE TABLE \`Property\` (\`Property\` CHAR(72) NOT NULL, \`Value\` CHAR(255) PRIMARY KEY \`Property\`)'
$v = $db.GetType().InvokeMember("OpenView", "InvokeMethod", $null, $db, @($sql))
$v.GetType().InvokeMember("Execute", "InvokeMethod", $null, $v, $null)
# Release each view before reopening the fixture database.
$v.GetType().InvokeMember("Close", "InvokeMethod", $null, $v, $null)
[Runtime.InteropServices.Marshal]::FinalReleaseComObject($v) | Out-Null
if ($FixtureName) {
  $sql = "INSERT INTO Property (Property, Value) VALUES ('ProductName', '$FixtureName')"
  $v = $db.GetType().InvokeMember("OpenView", "InvokeMethod", $null, $db, @($sql))
  $v.GetType().InvokeMember("Execute", "InvokeMethod", $null, $v, $null)
  $v.GetType().InvokeMember("Close", "InvokeMethod", $null, $v, $null)
  [Runtime.InteropServices.Marshal]::FinalReleaseComObject($v) | Out-Null
}
$db.GetType().InvokeMember("Commit", "InvokeMethod", $null, $db, $null)
[Runtime.InteropServices.Marshal]::FinalReleaseComObject($db) | Out-Null
$msi = Get-Item -LiteralPath $DatabasePath
${identity}
`)
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script, path.join(directory, 'product fixture.msi'), name], { encoding: 'utf8' })
    expect(result.status, result.stderr).toBe(status)
    expect(result.stdout + result.stderr).toContain(message)
    expect(result.stdout + result.stderr).not.toContain('DISP_E_TYPEMISMATCH')
  })
})

describe('Windows finalizer contract', { timeout: 30_000 }, () => { // A PowerShell spawn costs about 3.5 seconds, and the slowest observed test took 6993ms.
  const runnerPath = path.join(root, 'test/e2e/runner/windows.ps1')
  const runner = fs.readFileSync(runnerPath, 'utf8')
  const bodyBoundary = runner.indexOf('\ntry {\n  # desktop-ci')
  const finalizer = runner.slice(runner.indexOf('function Finalize-Run'), bodyBoundary)
  const phases = [...finalizer.matchAll(/Invoke-Cleanup "([^"]+)"/g)].map((match) => match[1])
  const injectablePhases = [...new Set(phases)].filter((phase) => phase !== 'suppress-artifacts')

  it('asserts installer registration and files before generic state removal', () => {
    expect(phases.indexOf('uninstall')).toBeLessThan(phases.indexOf('registration-gone'))
    expect(phases.indexOf('registration-gone')).toBeLessThan(phases.indexOf('installed-files-gone'))
    expect(phases.indexOf('installed-files-gone')).toBeLessThan(phases.indexOf('remove-state'))
    expect(finalizer).toMatch(/registration-gone[\s\S]+Assert-PerUserMsiRegistration \$snapshot.scope \$installLocalAppData -Absent/)
    expect(finalizer).toMatch(/installed-files-gone[^\n]+Test-Path -LiteralPath \$installDirectory/)
    expect(finalizer).toMatch(/processes-gone[\s\S]+Get-HarnessProcesses/)
    expect(runner).toMatch(/function Get-HarnessProcesses[\s\S]+Get-Process muniment/)
    expect(finalizer).toMatch(/Invoke-Cleanup "publish-artifacts"[^\n]+\n\s+if \(\$script:cleanupLastStatus -ne 0\) \{ Invoke-Cleanup "suppress-artifacts"/)
  })

  it('defaults the artifact directory to the path desktop-ci actually collects', () => {
    expect(runner).toContain('$artifacts = if ($env:DCI_ARTIFACTS_DIR) { $env:DCI_ARTIFACTS_DIR } else { Join-Path $env:TEMP "dci-artifacts" }')
    expect(runner).not.toContain('C:\\dci-artifacts')
  })

  it('starts diagnostics before the guarded body creates staging directories', () => {
    const boundary = bodyBoundary
    expect(runner.indexOf('New-Item -ItemType Directory -Force $artifacts')).toBeLessThan(runner.indexOf('Start-Transcript'))
    expect(runner.indexOf('Start-Transcript')).toBeLessThan(runner.indexOf('$ErrorActionPreference'))
    expect(runner.indexOf('New-Item -ItemType Directory -Force $artifacts')).toBeLessThan(runner.indexOf('$ErrorActionPreference'))
    expect(runner.indexOf('New-Item -ItemType Directory -Force $raw, $stateRoot')).toBeGreaterThan(boundary)
    expect(runner.indexOf('New-Item -ItemType File -Force $cleanupLog')).toBeGreaterThan(boundary)
    expect(runner).toContain('$diagnosticFile = Join-Path $artifacts "runner-failure.txt"')
    expect(runner).toMatch(/function Save-RunnerFailure[\s\S]+message:[\s\S]+category:[\s\S]+line:[\s\S]+Set-Content -LiteralPath \(Join-Path \$raw "runner-failure.txt"\)[\s\S]+Write-Output/)
    expect(runner).toContain('-Value $diagnostic -Encoding UTF8 -ErrorAction SilentlyContinue')
    expect(runner).toMatch(/Remove-Item Env:MUNIMENT_E2E_ONBOARDING_ONLY[^\n]+\n} catch \{\n  Save-RunnerFailure \$_/)
    expect(runner).toMatch(/try \{\n    Finalize-Run\n  } catch \{\n    Save-RunnerFailure \$_/)
    expect(runner).toContain('if ($diagnostic) {\n    # Publish the cause independently')
    expect(runner).toContain('Copy-Item -LiteralPath (Join-Path $diagnosticSafe "runner-failure.txt") -Destination $diagnosticFile -Force')
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

  it('Saves registration snapshots and an MSI log before the registration check.', () => {
    const install = runner.slice(runner.indexOf('function Install-Product'), runner.indexOf('function Get-HarnessProcesses'))
    expect(install.indexOf('Save-RegistrationSnapshot "before"')).toBeLessThan(install.indexOf('Invoke-BoundedProcess'))
    expect(install).toContain('/qn /norestart /L*V `"$msiLog`"')
    expect(install).toContain('(Join-Path $raw "installer-process.log")')
    expect(install).toMatch(/finally \{[\s\S]+Get-Content -LiteralPath \$msiLog -Raw[\s\S]+Set-Content -LiteralPath \(Join-Path \$raw "msi-verbose.log"\) -Encoding UTF8[\s\S]+Add-Content -LiteralPath \$installerLog -Encoding UTF8[\s\S]+Save-RegistrationSnapshot "after"/)
    expect(install.indexOf('Save-RegistrationSnapshot "after"')).toBeLessThan(install.indexOf('Assert-ProductRegistration $snapshot.scope'))
    const backup = runner.lastIndexOf('foreach ($name in @("installer.log", "msi-verbose.log", "registration-before.json", "registration-after.json"))')
    expect(backup).toBeGreaterThan(0)
    expect(backup).toBeLessThan(runner.lastIndexOf('\n    Finalize-Run'))
  })

  it('Reads both uninstall roots for each hive and publishes redacted evidence before the throw.', () => {
    const reader = runner.slice(runner.indexOf('function Get-UninstallEntries'), runner.indexOf('function Get-ProductRegistration'))
    expect(reader).not.toContain('if ($Hive -eq "HKLM") { return @() }')
    expect(reader).toContain('"$Hive`:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall"')
    expect(reader).toContain('"$Hive`:\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall"')
    const assertion = runner.slice(runner.indexOf('function Assert-ProductRegistration'), runner.indexOf('function Install-Product'))
    expect(assertion).toContain('Assert-PerUserMsiRegistration $Registration $installLocalAppData')
    expect(assertion.indexOf('Invoke-NativeCommand "node"')).toBeLessThan(assertion.indexOf('Copy-Item -Destination $artifacts'))
    expect(assertion.indexOf('Copy-Item -Destination $artifacts')).toBeLessThan(assertion.indexOf('\n    throw'))
  })

  it.skipIf(process.platform !== 'win32')('Walks both native registry roots for each hive outside fixture mode.', () => {
    const directory = temp()
    const script = path.join(directory, 'registry-reader.ps1')
    const reader = runner.slice(runner.indexOf('function Get-UninstallEntries'), runner.indexOf('function Get-ProductRegistration'))
    fs.writeFileSync(script, `
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
${reader}
function Test-Path { param($Path) return $true }
function Get-ChildItem { param($Path) return [pscustomobject]@{ PSChildName = 'product'; PSPath = "$Path\\product" } }
function Get-ItemProperty { param($Path, $ErrorAction) return [pscustomobject]@{ DisplayName = 'muniment' } }
@{ hkcu = @(Get-UninstallEntries 'HKCU'); hklm = @(Get-UninstallEntries 'HKLM') } | ConvertTo-Json -Depth 4 -Compress
`)
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script], {
      encoding: 'utf8', env: { ...process.env, MUNIMENT_E2E_FINALIZER_TEST_MODE: '0' },
    })
    expect(result.status, result.stdout + result.stderr).toBe(0)
    const inventory = JSON.parse(result.stdout)
    for (const Hive of ['HKCU', 'HKLM']) {
      expect(inventory[Hive.toLowerCase()].map((entry) => entry.PSPath)).toEqual([
        `${Hive}:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\product`,
        `${Hive}:\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\product`,
      ])
    }
  })

  const registrationFixture = (hkcu, hklm, valid = false) => {
    const fixture = path.join(temp(), 'registrations.json')
    const entries = (Hive, count) => Array.from({ length: count }, (_, index) => ({
      Hive, DisplayName: 'muniment', PSChildName: `${Hive}-product-${index}`,
      PSPath: `${Hive}:\\Software\\${index ? 'WOW6432Node\\' : ''}Microsoft\\Windows\\CurrentVersion\\Uninstall\\${Hive}-product-${index}`,
    }))
    const registrations = entries('HKCU', hkcu)
    const machineRegistrations = entries('HKLM', hklm)
    const inventory = [
      ...registrations, ...machineRegistrations,
      ...['HKCU', 'HKLM'].flatMap((Hive) => [
        { Hive, DisplayName: 'another product', PSChildName: 'unrelated', PSPath: `${Hive}:\\unrelated` },
        { Hive, DisplayName: null, PSChildName: 'unnamed', PSPath: `${Hive}:\\unnamed` },
      ]),
    ]
    fs.writeFileSync(fixture, JSON.stringify(inventory))
    const ProductCode = '{12345678-1234-ABCD-EF12-34567890ABCD}'
    const InstallLocation = `${path.dirname(fixture)}\\installed\\`
    const scope = {
      ProductCode, Sid: 'S-1-5-21-123', InstallLocation,
      keys: Object.fromEntries(['userProduct', 'userData', 'managed', 'machineProduct'].map((name) => [name, {
        path: `fixture-${name}`, present: name === 'userData' || (name === 'userProduct' && valid),
      }])),
      uninstall: {
        'HKCU:': registrations.map((entry) => ({ path: entry.PSPath, InstallLocation })),
        'Registry::HKEY_USERS\\S-1-5-21-123': registrations.map((entry) => ({ path: entry.PSPath, InstallLocation })),
        'HKLM:': machineRegistrations.map((entry) => ({ path: entry.PSPath, InstallLocation })),
      },
    }
    fs.writeFileSync(`${fixture}.scope.json`, JSON.stringify(scope))
    return { fixture, registrations, machineRegistrations }
  }

  const registrationMocks = runner.slice(
    runner.indexOf('    function Get-MsiProductCode {'),
    runner.indexOf('    function Invoke-BoundedProcess {', runner.indexOf('    function Get-MsiProductCode {')),
  )
  const registrationHelper = fs.readFileSync(path.join(root, 'test/windows-msi-registration.ps1'), 'utf8')

  const registrationMessage = 'Per-user MSI registration failed for ProductCode={12345678-1234-ABCD-EF12-34567890ABCD}: fixture-userProduct'

  it('Pins the ProductCode and the installer profile before the runner changes app state.', () => {
    expect(runner).toContain('$script:productCode = Get-MsiProductCode $msi')
    expect(runner.indexOf('$installLocalAppData = $env:LOCALAPPDATA')).toBeLessThan(runner.indexOf('$env:LOCALAPPDATA ='))
    expect(finalizer).toContain('"/x $productCode /qn /norestart"')
    expect(runner).toContain('$script:installDirectory = $snapshot.scope.InstallLocation')
    expect(runner).not.toContain('$registrations[0]')
    expect(runner).toContain("$_ -match '\\bMsiRunningElevated\\b'")
    expect(finalizer.indexOf('Write-PerUserMsiRegistration $snapshot.scope "uninstalled"')).toBeLessThan(finalizer.indexOf('Stop-Transcript'))
  })

  it('Loads the same registration checks in both installer paths.', () => {
    for (const source of [runner, fs.readFileSync(path.join(root, 'test/windows-installers.ps1'), 'utf8')]) {
      expect(source).toContain('windows-msi-registration.ps1')
      expect(source).toContain('Write-PerUserMsiRegistration')
      expect(source).toContain('Assert-PerUserMsiRegistration')
    }
  })

  it.skipIf(process.platform !== 'win32').each(
    [0, 1, 2].flatMap((hkcu) => [0, 1, 2]
      .flatMap((hklm) => ['', 'redact-artifacts', 'publish-artifacts'].map((failed) => [hkcu, hklm, failed]))),
  )('Publishes hkcu=%s hklm=%s after cleanup failure "%s".', (hkcu, hklm, failed) => {
    const { fixture, registrations, machineRegistrations } = registrationFixture(hkcu, hklm)
    const secret = 'installer-fixture-secret'
    const { result, artifacts, invoked, statuses } = runWindowsFinalizer(failed, '', {
      MUNIMENT_E2E_REGISTRATION_TEST_FIXTURE: fixture,
      MUNIMENT_E2E_PASSWORD: secret,
    })
    const detail = `hkcu=${hkcu} HKU\\S-1-5-21-123=${hkcu} hklm=${hklm}`
    const message = registrationMessage
    expect(result.status, result.stdout + result.stderr).toBe(1)
    expect(result.stdout).toContain(detail)
    expect(result.stdout).toContain('Property(S): MsiRunningElevated = 1')
    expect(result.stdout).toContain(message)
    expect(fs.readFileSync(path.join(artifacts, 'runner-failure.txt'), 'utf8')).toContain(`message: ${message}`)
    if (!failed) expect(fs.readFileSync(path.join(artifacts, 'runner-transcript.log'), 'utf8')).toContain(message)
    for (const name of ['installer.log', 'msi-verbose.log']) {
      const log = fs.readFileSync(path.join(artifacts, name), 'utf8')
      expect(log).toContain('MSI fixture: café [REDACTED]')
      expect(log).not.toContain(secret)
      expect(log).not.toContain('\0')
    }
    for (const phase of ['before', 'after']) {
      const snapshot = JSON.parse(fs.readFileSync(path.join(artifacts, `registration-${phase}.json`), 'utf8').replace(/^\uFEFF/, ''))
      expect(snapshot.hkcu).toEqual({ count: hkcu, entries: registrations })
      expect(snapshot.hklm).toEqual({ count: hklm, entries: machineRegistrations })
      expect(snapshot.userUninstallEntries).toHaveLength(hkcu + 2)
      expect(snapshot.machineUninstallEntries).toHaveLength(hklm + 2)
    }
    expect(invoked).toContain('remove-raw')
    expect(statuses.uninstall).toBe('0')
    expect(statuses['registration-gone']).toBe('0')
    if (failed === 'publish-artifacts') {
      expect(statuses['publish-artifacts']).toBe('1')
      expect(statuses['suppress-artifacts']).toBe('0')
    } else if (!failed) {
      expect(statuses['publish-artifacts']).toBe('0')
      expect(invoked).not.toContain('suppress-artifacts')
    }
    expect(fs.existsSync(path.join(artifacts, 'stale-or-partial'))).toBe(false)
    expect(fs.existsSync(path.join(artifacts, 'partial-publication'))).toBe(false)
  })

  it.skipIf(process.platform !== 'win32').each([0, 1, 2].flatMap((hkcu) => [0, 1, 2].map((hklm) => [hkcu, hklm])))('Accepts per-user scope with hkcu=%s and hklm=%s.', (hkcu, hklm) => {
    const { fixture } = registrationFixture(hkcu, hklm, true)
    const { result, artifacts } = runWindowsFinalizer('', '', { MUNIMENT_E2E_REGISTRATION_TEST_FIXTURE: fixture })
    expect(result.status, result.stdout + result.stderr).toBe(0)
    expect(result.stdout).toContain(`hkcu=${hkcu} HKU\\S-1-5-21-123=${hkcu} hklm=${hklm}`)
    const transcript = fs.readFileSync(path.join(artifacts, 'runner-transcript.log'), 'utf8')
    expect(transcript).toContain('per-user MSI uninstalled ProductCode=')
    for (const key of ['userProduct', 'userData', 'managed', 'machineProduct']) {
      expect(transcript).toContain(`fixture-${key} present=0`)
    }
    expect(transcript).toContain('hkcu=0 HKU\\S-1-5-21-123=0 hklm=0')
    expect(fs.existsSync(path.join(artifacts, 'runner-failure.txt'))).toBe(false)
  })

  it.skipIf(process.platform !== 'win32').each([false, true])('Publishes evidence before the assertion throws with an empty fixture: %s.', (empty) => {
    const directory = temp()
    const script = path.join(directory, 'registration-assertion.ps1')
    const { fixture } = registrationFixture(0, 1)
    if (empty) {
      fs.writeFileSync(fixture, '[]')
      const scope = JSON.parse(fs.readFileSync(`${fixture}.scope.json`, 'utf8'))
      for (const hive of Object.keys(scope.uninstall)) scope.uninstall[hive] = []
      fs.writeFileSync(`${fixture}.scope.json`, JSON.stringify(scope))
    }
    const definitions = runner.slice(runner.indexOf('function Resolve-NativeCommand'), runner.indexOf('function Get-HarnessProcesses'))
    fs.writeFileSync(script, `
param($runRoot, $redactor)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$raw = Join-Path $runRoot 'raw'
$safe = Join-Path $runRoot 'safe'
$artifacts = Join-Path $runRoot 'artifacts'
$cleanupLog = Join-Path $runRoot 'cleanup.log'
$installerLog = Join-Path $raw 'installer.log'
$msi = Join-Path $runRoot 'fixture.msi'
New-Item -ItemType Directory -Force $raw, $artifacts | Out-Null
${registrationHelper}
${definitions}
${registrationMocks}
$installLocalAppData = Split-Path -Parent $env:MUNIMENT_E2E_REGISTRATION_TEST_FIXTURE
function Invoke-BoundedProcess {
  Set-Content -LiteralPath (Join-Path $runRoot 'msi-verbose.log') -Value "MSI fixture: $env:MUNIMENT_E2E_PASSWORD" -Encoding Unicode
}
try { Install-Product } catch { Write-Output $_.Exception.Message; exit 1 }
exit 0
`)
    const secret = 'registration-publication-secret'
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script,
      directory, path.join(root, 'test/e2e/support/redact.mjs')], {
      encoding: 'utf8', env: { ...process.env, MUNIMENT_E2E_FINALIZER_TEST_MODE: '1',
        MUNIMENT_E2E_REGISTRATION_TEST_FIXTURE: fixture, MUNIMENT_E2E_PASSWORD: secret },
    })
    expect(result.status, result.stdout + result.stderr).toBe(1)
    expect(result.stdout).toContain(registrationMessage)
    expect(result.stdout).toContain(`hkcu=0 HKU\\S-1-5-21-123=0 hklm=${empty ? 0 : 1}`)
    for (const name of ['registration-before.json', 'registration-after.json', 'msi-verbose.log']) {
      const text = fs.readFileSync(path.join(directory, 'artifacts', name), 'utf8')
      expect(text).not.toContain(secret)
      if (name.endsWith('.json')) {
        const snapshot = JSON.parse(text.replace(/^\uFEFF/, ''))
        expect(snapshot.hkcu.count).toBe(0)
        expect(snapshot.hklm.count).toBe(empty ? 0 : 1)
      } else {
        expect(text).toContain('MSI fixture: [REDACTED]')
        expect(text).not.toContain('\0')
      }
    }
  })

  it.skipIf(process.platform !== 'win32').each(['', 'redact-artifacts', 'publish-artifacts'])(
    'Preserves the terminating cause after cleanup failure "%s".', (failed) => {
      const cause = 'Type mismatch. (Exception from HRESULT: 0x80020005 (DISP_E_TYPEMISMATCH))'
      const secret = 'injected-runner-secret'
      const { result, artifacts } = runWindowsFinalizer(failed, '', {
        MUNIMENT_E2E_RUNNER_TEST_ERROR: `${cause} ${secret}`,
        MUNIMENT_E2E_PASSWORD: secret,
      })
      expect(result.status, result.stderr).toBe(1)
      expect(result.stdout).not.toContain('=== DESKTOP-CI ARTIFACTS BEGIN ===')
      const diagnostic = fs.readFileSync(path.join(artifacts, 'runner-failure.txt'), 'utf8')
      expect(diagnostic).toContain(`message: ${cause} [REDACTED]`)
      expect(diagnostic).not.toContain(secret)
      expect(diagnostic).toMatch(/category: \w+/)
      expect(diagnostic).toMatch(/line: [1-9]\d*/)
      expect(fs.existsSync(path.join(artifacts, 'stale-or-partial'))).toBe(false)
      expect(fs.existsSync(path.join(artifacts, 'partial-publication'))).toBe(false)
    },
  )

  const powershell = process.platform === 'win32' ? 'powershell.exe' : 'pwsh'
  const hasPowerShell = spawnSync(powershell, ['-NoProfile', '-Command', 'exit 0'], { timeout: 15_000 }).status === 0

  it.skipIf(!hasPowerShell).each(['missing', 'directory', 'file'])('The bootstrap checks the %s artifact path before dependency installation.', (state) => {
    const directory = temp()
    const artifacts = path.join(directory, 'artifacts [fixture]')
    if (state === 'file') fs.writeFileSync(artifacts, 'blocked')
    if (state === 'directory') fs.mkdirSync(artifacts)
    const script = path.join(directory, 'bootstrap.ps1')
    const bootstrap = runner.slice(0, runner.indexOf('$diagnostic = $null'))
    fs.writeFileSync(script, `${bootstrap}\nStop-Transcript -ErrorAction SilentlyContinue | Out-Null\nWrite-Output 'Bootstrap reached dependency installation.'\n`)
    const result = spawnSync(powershell, ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script], {
      encoding: 'utf8', timeout: 15_000,
      env: { ...process.env, TEMP: directory, TMP: directory, DCI_ARTIFACTS_DIR: artifacts },
    })
    expect(result.status, result.stdout + result.stderr).toBe(state === 'file' ? 1 : 0)
    expect(result.stdout.includes('Bootstrap reached dependency installation.')).toBe(state !== 'file')
    if (state === 'file') {
      expect(result.stdout).toContain(`message: The artifact path is not a directory: ${artifacts}`)
      expect(fs.readFileSync(artifacts, 'utf8')).toBe('blocked')
    } else {
      expect(fs.statSync(artifacts).isDirectory()).toBe(true)
    }
  })

  it.skipIf(process.platform !== 'win32')('writes stdout when artifact directory creation fails', () => {
    const directory = temp()
    const blockedPath = path.join(directory, 'not-a-directory')
    const cli = path.join(root, 'node_modules/@tauri-apps/cli/tauri.js')
    const installedCli = fs.readFileSync(cli)
    fs.writeFileSync(blockedPath, 'blocked')
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', runnerPath], {
      encoding: 'utf8', timeout: 15_000,
      env: { ...process.env, TEMP: directory, TMP: directory, DCI_ARTIFACTS_DIR: blockedPath },
    })
    expect(result.status).not.toBe(0)
    expect(result.stdout).toContain(`message: The artifact path is not a directory: ${blockedPath}`)
    expect(result.stdout).toContain('category:')
    expect(result.stdout).toMatch(/line: [1-9]\d*/)
    expect(result.stdout + result.stderr).not.toContain('npm dependency installation')
    expect(fs.readFileSync(cli)).toEqual(installedCli)
    expect(fs.readFileSync(blockedPath, 'utf8')).toBe('blocked')
  })

  it.skipIf(process.platform !== 'win32')('writes a diagnostic artifact and stdout when transcript startup fails', () => {
    const directory = temp()
    const artifacts = path.join(directory, 'artifacts')
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', runnerPath], {
      encoding: 'utf8',
      env: { ...process.env, TEMP: directory, TMP: directory, DCI_ARTIFACTS_DIR: artifacts, MUNIMENT_E2E_BOOTSTRAP_TEST_FAIL: 'start-transcript' },
    })
    expect(result.status).not.toBe(0)
    expect(result.stdout).toContain('message: injected Start-Transcript failure')
    expect(fs.readFileSync(path.join(artifacts, 'runner-failure.txt'), 'utf8')).toContain('message: injected Start-Transcript failure')
  })

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
    const { result, invoked, artifacts } = runWindowsFinalizer('', 'before-directories')
    expect(result.status).not.toBe(0)
    expect(result.stdout).toContain('message: injected setup failure')
    expect(result.stdout).toContain('dci: Windows runner transcript tail')
    const diagnostic = fs.readFileSync(path.join(artifacts, 'runner-failure.txt'), 'utf8')
    expect(diagnostic).toContain('message: injected setup failure')
    expect(diagnostic).toMatch(/category: \w+/)
    expect(diagnostic).toMatch(/line: [1-9]\d*/)
    expect(invoked).toContain('redact-artifacts')
    expect(invoked).toContain('suppress-artifacts')
    expect(invoked.at(-1)).toBe('suppress-artifacts')
  })

  it.skipIf(process.platform !== 'win32')('publishes a minimal report after redaction failure', () => {
    const { result, artifacts, directory, invoked } = runWindowsFinalizer('redact-artifacts')
    expect(result.status).not.toBe(0)
    expect(invoked).toContain('suppress-artifacts')
    expect(fs.readdirSync(artifacts).sort()).toEqual(['cleanup-status.log', 'envelope-reason.txt', 'redaction-failure.txt'])
    expect(fs.readFileSync(path.join(artifacts, 'envelope-reason.txt'), 'utf8')).toContain('reason: redaction-failed')
    const report = fs.readFileSync(path.join(artifacts, 'redaction-failure.txt'), 'utf8').replaceAll('\r\n', '\n')
    expect(report).toBe('file: unknown\ncategory: redactor-process\n')
    expect(fs.readdirSync(directory).filter((name) => name.startsWith('muniment-e2e-'))).toEqual([])
  })

  it.skipIf(process.platform !== 'win32')('redacts an injected secret from the transcript', () => {
    const plantedSecret = 'windows-planted-secret'
    const { result, artifacts } = runWindowsFinalizer('', '', {
      MUNIMENT_E2E_PASSWORD: plantedSecret,
      MUNIMENT_E2E_FINALIZER_TEST_TRANSCRIPT_TEXT: plantedSecret,
    })
    expect(result.status).toBe(0)
    const transcript = fs.readFileSync(path.join(artifacts, 'runner-transcript.log'), 'utf8')
    expect(transcript).toContain('[REDACTED]')
    expect(transcript).not.toContain(plantedSecret)
  })

  it.skipIf(process.platform !== 'win32')('destroys staging and prints the transcript after publication failure', () => {
    const { result, artifacts, directory, invoked } = runWindowsFinalizer('publish-artifacts')
    expect(result.status).not.toBe(0)
    expect(invoked).toContain('suppress-artifacts')
    expect(result.stdout).toContain('dci: Windows runner transcript tail')
    expect(fs.readdirSync(artifacts)).toEqual([])
    expect(fs.readdirSync(directory).filter((name) => name.startsWith('muniment-e2e-'))).toEqual([])
  })
})

describe('Windows nightly workflow gate', () => {
  const workflow = fs.readFileSync(path.join(root, '.github/workflows/nightly.yml'), 'utf8')
  const job = workflow.slice(workflow.indexOf('\n  windows-e2e:'), workflow.indexOf('\n    runs-on:', workflow.indexOf('\n  windows-e2e:')))
  const condition = job.match(/\n    if: >-\n([\s\S]+)$/)?.[1].trim().replace(/\n\s*/g, ' ')
  const evaluate = ({ eventName, platform, cancelled = false, prepare = 'success', linux = 'success' }) => Function(
    'always', 'cancelled', 'needs', 'github',
    `return ${condition.replaceAll('needs.linux-e2e', 'needs.linuxE2e')}`,
  )(() => true, () => cancelled, { prepare: { result: prepare }, linuxE2e: { result: linux } }, { event_name: eventName, event: { inputs: { platform } } })

  it.each([
    ['schedule', undefined],
    ['workflow_dispatch', 'all'],
  ])('runs after Linux for a full %s nightly', (eventName, platform) => {
    expect(evaluate({ eventName, platform })).toBe(true)
  })

  it('excludes a Linux-only dispatch', () => {
    expect(evaluate({ eventName: 'workflow_dispatch', platform: 'linux' })).toBe(false)
  })

  it('runs a Windows-only dispatch when Linux skips', () => {
    expect(evaluate({ eventName: 'workflow_dispatch', platform: 'windows', linux: 'skipped' })).toBe(true)
  })

  it.each([
    ['schedule', undefined, 'failure', 'success'],
    ['schedule', undefined, 'success', 'skipped'],
  ])('does not run without the full serialized prerequisites', (eventName, platform, prepare, linux) => {
    expect(evaluate({ eventName, platform, prepare, linux })).toBe(false)
  })
})

describe.skipIf(process.platform === 'win32')('Linux early abort reporting', () => {
  const runnerPath = path.join(root, 'test/e2e/runner/linux.sh')
  const runner = fs.readFileSync(runnerPath, 'utf8')

  it('reports every missing injected variable without exposing injected values', () => {
    const directory = temp()
    const artifacts = path.join(directory, 'artifacts')
    const plantedSecret = 'linux-planted-secret'
    const env = {
      ...process.env,
      DCI_ARTIFACTS_DIR: artifacts,
      MUNIMENT_E2E_SOURCE_SHA: 'a'.repeat(40),
      GH_TOKEN: plantedSecret,
      MUNIMENT_E2E_USERNAME: plantedSecret,
      MUNIMENT_E2E_PASSWORD: '',
    }
    const result = spawnSync('bash', [runnerPath], { encoding: 'utf8', env })
    expect(result.status).not.toBe(0)
    const reason = fs.readFileSync(path.join(artifacts, 'runner-failure.txt'), 'utf8')
    expect(reason).toContain('missing injected environment: MUNIMENT_E2E_PASSWORD (repository secret DESKTOP_E2E_PASSWORD)')
    expect(reason).not.toContain('MUNIMENT_E2E_PROVIDER_KEY')
    expect(reason).not.toContain('GH_TOKEN')
    expect(reason).not.toContain('MUNIMENT_E2E_USERNAME')
    expect(`${reason}\n${result.stderr}`).not.toContain(plantedSecret)

    const junit = spawnSync('bash', [path.join(root, 'test/e2e/support/ensure-junit-report.sh'), artifacts, 'installed-linux', '1', '0'], { encoding: 'utf8' })
    expect(junit.status, junit.stderr).toBe(0)
    const report = fs.readFileSync(path.join(artifacts, 'junit-infrastructure.xml'), 'utf8')
    expect(report).not.toContain('MUNIMENT_E2E_PROVIDER_KEY')
    expect(report).not.toContain('DESKTOP_E2E_PROVIDER_KEY')
    expect(report).not.toContain(plantedSecret)
  })

  it('routes every printed early abort through the reason helper', () => {
    const earlyRun = runner.slice(runner.indexOf('\nsha=${MUNIMENT_E2E_SOURCE_SHA:-}'), runner.indexOf('\nready=1'))
    const bypasses = [...earlyRun.matchAll(/\{([^{}]*status=1; exit;[^{}]*)\}/g)]
      .filter((match) => /\becho\b/.test(match[1]) && !/\brunner_failure\b/.test(match[1]))
    expect(bypasses).toEqual([])
    expect(earlyRun).not.toContain("echo 'required injected environment is unavailable'")
    expect(earlyRun).toContain("runner_failure 'invalid source SHA'")
  })
})

describe.skipIf(process.platform === 'win32')('runner setup causes', () => {
  it.each(['linux', 'macos'])('carries a missing %s asset through redaction into JUnit', (platform) => {
    const directory = temp()
    const artifacts = path.join(directory, 'artifacts')
    const bin = path.join(directory, 'bin')
    fs.mkdirSync(bin)
    for (const [name, content] of Object.entries({
      gh: '#!/bin/sh\nprintf \'{"assets":[{"name":"other-platform.zip","id":1}]}\\n\'\n',
      stat: '#!/bin/sh\necho test-user\n',
      id: '#!/bin/sh\necho test-user\n',
    })) {
      fs.writeFileSync(path.join(bin, name), content, { mode: 0o700 })
    }
    const sha = 'a'.repeat(40)
    const result = spawnSync('bash', [path.join(root, `test/e2e/runner/${platform}.sh`)], {
      encoding: 'utf8',
      env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, DCI_ARTIFACTS_DIR: artifacts,
        MUNIMENT_E2E_SOURCE_SHA: sha, GH_TOKEN: 'injected-token', GITHUB_REPOSITORY: 'test/repo',
        MUNIMENT_E2E_USERNAME: 'injected-user', MUNIMENT_E2E_PASSWORD: 'injected-password' },
    })
    expect(result.status).toBe(1)
    const cause = fs.readFileSync(path.join(artifacts, 'runner-failure.txt'), 'utf8')
    const expected = `nightly-${sha}-${platform}-${platform === 'macos' ? 'muniment.app.zip' : 'muniment.deb'}`
    expect(cause).toContain(expected)
    expect(cause).toContain('matches=0, release assets=["other-platform.zip"]')
    const junit = spawnSync('bash', [path.join(root, 'test/e2e/support/ensure-junit-report.sh'), artifacts, `installed-${platform}`, '1', '0'], { encoding: 'utf8' })
    expect(junit.status, junit.stderr).toBe(0)
    const report = fs.readFileSync(path.join(artifacts, 'junit-infrastructure.xml'), 'utf8')
    expect(report).toContain(expected)
    expect(report).toContain('matches=0, release assets=[&quot;other-platform.zip&quot;]')
    expect(report).not.toContain('desktop-ci failed before producing a JUnit report')
  })

  it.each(['stdout', 'both', 'empty'])('carries %s setup errors into JUnit', (output) => {
    const raw = temp()
    const fatal = 'fatal: installer rejected package signature'
    const result = spawnSync('bash', ['-c', `
      source test/e2e/support/runner-failure.sh
      raw=$1
      status=0
      run_setup bash -c '${output !== 'empty' ? `printf "${fatal}";` : ''} ${output === 'both' ? 'echo warning >&2;' : ''} exit 7'
      exit "$?"
    `, 'bash', raw], { encoding: 'utf8' })
    expect(result.status).toBe(7)
    expect(result.stdout).toBe(output === 'empty' ? '' : fatal)
    const cause = fs.readFileSync(path.join(raw, 'runner-failure.txt'), 'utf8')
    expect(cause).toBe(`bash failed (exit code 7)${output === 'empty' ? '' : `: ${fatal}`}${output === 'both' ? '\nwarning' : ''}\n`)
    const junit = spawnSync('bash', [path.join(root, 'test/e2e/support/ensure-junit-report.sh'), raw, 'installed-linux', '1', '0'], { encoding: 'utf8' })
    expect(junit.status, junit.stderr).toBe(0)
    expect(fs.readFileSync(path.join(raw, 'junit-infrastructure.xml'), 'utf8'))
      .toContain(`<failure message="${cause.trim().replaceAll('\n', ' ')}"/>`)
  })

  it.each(['stdout', 'stderr'])('keeps both streams in JUnit with long %s progress', (stream) => {
    const raw = temp()
    const progress = 'p'.repeat(1800)
    const fatal = 'fatal: installer rejected package signature'
    const stdout = stream === 'stdout' ? progress : fatal
    const stderr = stream === 'stderr' ? progress : fatal
    const result = spawnSync('bash', ['-c', `
      source test/e2e/support/runner-failure.sh
      raw=$1
      status=0
      run_setup bash -c 'printf "%s" "$1"; printf "%s" "$2" >&2; exit 7' bash "$2" "$3"
    `, 'bash', raw, stdout, stderr], { encoding: 'utf8' })
    expect(result.status).toBe(7)
    expect(result.stdout).toBe(stdout)
    expect(result.stderr).toContain(stderr)
    expect(fs.readFileSync(path.join(raw, 'setup-stdout.log'), 'utf8')).toBe(stdout)
    expect(fs.readFileSync(path.join(raw, 'setup-stderr.log'), 'utf8')).toBe(stderr)
    const cause = fs.readFileSync(path.join(raw, 'runner-failure.txt'), 'utf8')
    expect(cause.length).toBeLessThanOrEqual(1000)
    const junit = spawnSync('bash', [path.join(root, 'test/e2e/support/ensure-junit-report.sh'), raw, 'installed-linux', '1', '0'], { encoding: 'utf8' })
    expect(junit.status, junit.stderr).toBe(0)
    const report = fs.readFileSync(path.join(raw, 'junit-infrastructure.xml'), 'utf8')
    expect(report).toContain('bash failed (exit code 7)')
    expect(report).toContain(fatal)
    expect(report).toContain('p'.repeat(100))
    expect(report).toContain(' ... ')
  })

  it('preserves successful binary output and stderr without a cause file', () => {
    const raw = temp()
    const result = spawnSync('bash', ['-c', `
      source test/e2e/support/runner-failure.sh
      raw=$1
      status=0
      run_setup bash -c 'printf "a\\\\0b\\\\n\\\\n"; printf warning >&2'
    `, 'bash', raw])
    expect(result.status).toBe(0)
    expect(result.stdout).toEqual(Buffer.from('a\0b\n\n'))
    expect(result.stderr.toString()).toBe('warning')
    expect(fs.existsSync(path.join(raw, 'runner-failure.txt'))).toBe(false)
    expect(fs.existsSync(path.join(raw, 'setup-stdout.log'))).toBe(false)
  })

  it('keeps command output separate and records only failures', () => {
    const raw = temp()
    const result = spawnSync('bash', ['-c', `
      source test/e2e/support/runner-failure.sh
      raw=$1
      status=0
      run_setup bash -c 'echo result; echo warning >&2'
      test ! -e "$raw/runner-failure.txt" || exit 2
      run_setup bash -c 'echo "lookup <failed> & stopped" >&2; exit 7'
      exit "$?"
    `, 'bash', raw], { encoding: 'utf8' })
    expect(result.status).toBe(7)
    expect(result.stdout).toBe('result\n')
    expect(fs.readFileSync(path.join(raw, 'runner-failure.txt'), 'utf8'))
      .toBe('bash failed (exit code 7): lookup <failed> & stopped\n')
  })
})

describe('JUnit infrastructure fallback', () => {
  it.skipIf(process.platform === 'win32')('includes and escapes the captured runner reason', () => {
    const artifacts = temp()
    fs.writeFileSync(path.join(artifacts, 'runner-failure.txt'), 'message: setup <failed> & stopped\ncategory: InvalidOperation\nline: 42\n')
    const result = spawnSync('bash', [path.join(root, 'test/e2e/support/ensure-junit-report.sh'), artifacts, 'installed-windows', '1', '1'], { encoding: 'utf8' })
    expect(result.status, result.stderr).toBe(0)
    const report = fs.readFileSync(path.join(artifacts, 'junit-infrastructure.xml'), 'utf8')
    expect(report).toContain('<failure message="message: setup &lt;failed&gt; &amp; stopped category: InvalidOperation line: 42"/>')
    expect(report).not.toContain('desktop-ci did not return a valid artifact envelope')
  })

  it.skipIf(process.platform === 'win32')('Reports a runner error without replacing an existing spec report.', () => {
    const artifacts = temp()
    const specs = '<testsuites tests="1" failures="0"/>'
    fs.writeFileSync(path.join(artifacts, 'junit-specs.xml'), specs)
    fs.writeFileSync(path.join(artifacts, 'runner-failure.txt'), 'message: Windows onboarding tests failed\n')
    const result = spawnSync('bash', [path.join(root, 'test/e2e/support/ensure-junit-report.sh'), artifacts, 'installed-windows', '1', '0'], { encoding: 'utf8' })
    expect(result.status, result.stderr).toBe(0)
    expect(fs.readFileSync(path.join(artifacts, 'junit-specs.xml'), 'utf8')).toBe(specs)
    expect(fs.readFileSync(path.join(artifacts, 'junit-infrastructure.xml'), 'utf8'))
      .toContain('<failure message="message: Windows onboarding tests failed"/>')
  })

  it.skipIf(process.platform === 'win32').each([
    ['1', '0', ''],
    ['0', '1', ''],
    ['0', '0', 'stale cause'],
  ])('Preserves an existing report for statuses %s/%s and cause "%s".', (runStatus, extractStatus, cause) => {
    const artifacts = temp()
    const specs = '<testsuites tests="1" failures="0"/>'
    fs.writeFileSync(path.join(artifacts, 'junit-specs.xml'), specs)
    fs.writeFileSync(path.join(artifacts, 'runner-failure.txt'), cause)
    const result = spawnSync('bash', [path.join(root, 'test/e2e/support/ensure-junit-report.sh'), artifacts, 'installed-windows', runStatus, extractStatus, '1'], { encoding: 'utf8' })
    expect(result.status, result.stderr).toBe(0)
    expect(fs.readFileSync(path.join(artifacts, 'junit-specs.xml'), 'utf8')).toBe(specs)
    expect(fs.readdirSync(artifacts).filter((name) => name.endsWith('.xml'))).toEqual(['junit-specs.xml'])
  })

  it.skipIf(process.platform === 'win32')('does not copy the desktop-ci transcript into JUnit', () => {
    const artifacts = temp()
    const transcript = '-----DESKTOP-CI-ARTIFACTS-BEGIN-----\nc2Vuc2l0aXZl\n-----DESKTOP-CI-ARTIFACTS-END-----\n'
    fs.writeFileSync(path.join(artifacts, 'desktop-ci-harness.log'), transcript)
    const result = spawnSync('bash', [path.join(root, 'test/e2e/support/ensure-junit-report.sh'), artifacts, 'installed-linux', '1', '0'], { encoding: 'utf8' })
    expect(result.status, result.stderr).toBe(0)
    const report = fs.readFileSync(path.join(artifacts, 'junit-infrastructure.xml'), 'utf8')
    expect(report).toContain('<failure message="desktop-ci failed before producing a JUnit report"/>')
    expect(report).not.toContain('c2Vuc2l0aXZl')
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
    ['unapproved screenshot', { 'failure-current-window.png': Buffer.from('not safe') }, {}],
    ['rendered production conversation', { '03-chat-complete.png': Buffer.from('not safe') }, {}],
  ])('blocks %s before destination creation', (_name, files, env) => {
    const { result, destination } = redact(files, env)
    expect(result.status).not.toBe(0); expect(fs.existsSync(destination)).toBe(false)
  })
  it.each([
    ['credential-header', 'Authorization: Basic abcdefghijklmnopqrstuvwxyz'],
    ['bearer-token', 'Bearer abcdefghijklmnopqrstuvwxyz'],
    ['oauth-token', 'id_token=abcdef'],
    ['github-token', 'ghp_abcdefghijklmnopqrstuvwxyz'],
    ['jwt', 'eyJheader.eyJpayload.signature'],
  ])('redacts %s content and names the category', (category, content) => {
    const { result, destination } = redact({ 'page-source-sign-in.html': content })
    expect(result.status).toBe(0)
    expect(fs.readFileSync(path.join(destination, 'page-source-sign-in.html'), 'utf8')).toBe(`[REDACTED:${category}]`)
  })
  it('redacts injected values from text logs', () => {
    const { result, destination } = redact(
      { 'real-sign-in.spec-0-0.log': 'before private and private-password after' },
      { MUNIMENT_E2E_USERNAME: 'private', MUNIMENT_E2E_PASSWORD: 'private-password' },
    )
    expect(result.status, result.stderr).toBe(0)
    expect(fs.readFileSync(path.join(destination, 'real-sign-in.spec-0-0.log'), 'utf8')).toBe('before [REDACTED] and [REDACTED] after')
  })
  it('blocks an injected value hidden in an approved screenshot file', () => {
    const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=', 'base64')
    const { result, destination } = redact({ '01-signed-out.png': Buffer.concat([png, Buffer.from('private-user')]) }, { MUNIMENT_E2E_USERNAME: 'private-user' })
    expect(result.status).not.toBe(0); expect(fs.existsSync(destination)).toBe(false)
  })
  it('strips harmless screenshot metadata', () => {
    const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=', 'base64')
    const metadata = Buffer.alloc(16)
    metadata.writeUInt32BE(4); metadata.write('tEXt', 4); metadata.write('test', 8)
    const input = Buffer.concat([png.subarray(0, 33), metadata, png.subarray(33)])
    const { result, destination } = redact({ 'screenshot-cleanup.png': input })
    expect(result.status, result.stderr).toBe(0)
    expect(fs.readFileSync(path.join(destination, 'screenshot-cleanup.png'))).toEqual(png)
  })
  it('blocks an unknown critical screenshot chunk', () => {
    const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=', 'base64')
    const chunk = Buffer.alloc(12)
    chunk.write('ABCD', 4)
    const input = Buffer.concat([png.subarray(0, 33), chunk, png.subarray(33)])
    const { result, destination } = redact({ 'screenshot-cleanup.png': input })
    expect(result.status).not.toBe(0); expect(fs.existsSync(destination)).toBe(false)
  })
})

// This block runs a POSIX shell script, and Windows has no shell for it.
describe.skipIf(process.platform === 'win32')('desktop-ci payload extraction', () => {
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

  it('Carries a terminating Windows error through the driver envelope into JUnit.', () => {
    const cause = 'message: Exception calling "InvokeMember" with "5" argument(s): "Type mismatch. (Exception from HRESULT: 0x80020005 (DISP_E_TYPEMISMATCH))"\r\ncategory: NotSpecified\r\nline: 355\r\n'
    const { result, destination } = extractInto(
      `${cause}[desktop-ci] BUILD FAILED (windows) rc=1\n${driverMarkers(logArchive('runner-failure.txt', `\ufeff${cause}`))}`,
      1,
    )
    expect(result.status, result.stderr).toBe(0)
    expect(fs.readFileSync(path.join(destination, 'runner-failure.txt'), 'utf8')).toContain(cause)
    const junit = spawnSync('bash', [path.join(root, 'test/e2e/support/ensure-junit-report.sh'), destination, 'installed-windows', '1', String(result.status)], { encoding: 'utf8' })
    expect(junit.status, junit.stderr).toBe(0)
    const report = fs.readFileSync(path.join(destination, 'junit-infrastructure.xml'), 'utf8')
    expect(report).toContain('Type mismatch. (Exception from HRESULT: 0x80020005 (DISP_E_TYPEMISMATCH))')
    expect(report).toContain('&quot;InvokeMember&quot;')
    expect(report).not.toContain('desktop-ci did not return a valid artifact envelope')
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

// This block runs a POSIX shell script, and Windows has no shell for it.
describe.skipIf(process.platform === 'win32')('cleanup failure accounting', () => {
  const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/linux.sh'), 'utf8')
  const phases = ['stop-wdio', 'revoke-session', 'stop-app', 'remove-package', 'remove-state', 'package-gone', 'processes-gone', 'state-gone', 'stage-cleanup-log', 'redact-artifacts', 'remove-raw', 'remove-package-file', 'remove-auth-url', 'replace-artifacts', 'publish-artifacts', 'suppress-artifacts', 'remove-safe', 'raw-gone', 'package-file-gone', 'auth-url-gone', 'safe-gone', 'remove-cleanup-log']
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
  it('checks the installed release before selecting the feature build', () => {
    expect(runner).toContain('release_binary=$(command -v muniment-desktop || command -v muniment)')
    expect(runner.indexOf('webdriver-release-guard.mjs absent')).toBeLessThan(runner.indexOf('webdriver-artifact-guard.sh present'))
  })
  it('finds a WebDriver marker inside a compressed DEB payload', () => {
    const fixture = temp()
    const payload = path.join(fixture, 'payload')
    const packageRoot = path.join(fixture, 'package')
    fs.mkdirSync(path.join(payload, 'usr', 'bin'), { recursive: true })
    fs.mkdirSync(packageRoot)
    fs.writeFileSync(path.join(payload, 'usr', 'bin', 'muniment-desktop'), 'TAURI_WEBDRIVER_PORT')
    fs.writeFileSync(path.join(packageRoot, 'debian-binary'), '2.0\n')
    const data = spawnSync('tar', ['-czf', path.join(packageRoot, 'data.tar.gz'), '-C', payload, '.'], { encoding: 'utf8' })
    expect(data.status, data.stderr).toBe(0)
    const controlRoot = path.join(fixture, 'control')
    fs.mkdirSync(controlRoot)
    fs.writeFileSync(path.join(controlRoot, 'control'), 'Package: muniment\nVersion: 1.0.0\nArchitecture: amd64\n')
    const control = spawnSync('tar', ['-czf', path.join(packageRoot, 'control.tar.gz'), '-C', controlRoot, '.'], { encoding: 'utf8' })
    expect(control.status, control.stderr).toBe(0)
    const deb = path.join(fixture, 'muniment.deb')
    const archive = spawnSync('ar', ['r', deb, 'debian-binary', 'control.tar.gz', 'data.tar.gz'], { cwd: packageRoot, encoding: 'utf8' })
    expect(archive.status, archive.stderr).toBe(0)

    const guard = spawnSync('bash', [path.join(root, 'test/e2e/support/webdriver-artifact-guard.sh'), 'present', deb], { cwd: root, encoding: 'utf8' })
    expect(guard.status, guard.stderr).toBe(0)
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
    const expectedFiles = failed === 'redact-artifacts'
      ? ['cleanup-status.log', 'envelope-reason.txt', 'redaction-failure.txt']
      : ['cleanup-status.log', 'envelope-reason.txt']
    expect(fs.readdirSync(extracted).sort()).toEqual(expectedFiles)
    expect(fs.readFileSync(path.join(extracted, 'envelope-reason.txt'), 'utf8')).toContain(`reason: ${reason}`)
    if (failed === 'redact-artifacts') {
      expect(fs.readFileSync(path.join(extracted, 'redaction-failure.txt'), 'utf8')).toBe('file: unknown\ncategory: redactor-process\n')
    }
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
    expect(invoked.slice(0, 5)).toEqual(['stop-wdio', 'revoke-session', 'stop-app', 'remove-package', 'remove-state'])
    expect(command['stop-wdio']).toBe("stop_matching \\[w\\]dio.\\\*test/e2e/wdio.conf.js ")
    expect(command['revoke-session']).toBe('run_cleanup_e2e ')
    expect(command['stop-app']).toBe('stop_app ')
    expect(command['remove-package']).toBe('sudo apt-get remove -y muniment ')
    expect(command['package-gone']).toBe('package_absent ')
    expect(command['processes-gone']).toBe('harness_processes_gone ')

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
    expect(command['redact-artifacts']).toMatch(new RegExp(`^node test/e2e/support/redact\\.mjs ${raw} ${safe} /tmp/muniment-e2e-redaction\\.[^ ]+\\.log $`))
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

describe('installed ACP adapter contract', () => {
  it('probes the installed adapter after its executability check', () => {
    const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/linux.sh'), 'utf8')
    const executableCheck = '[[ -x /usr/lib/muniment/muniment-acp ]]'
    const probe = 'node test/e2e/support/probe-installed-adapter.mjs /usr/lib/muniment/muniment-acp'
    expect(runner).toContain(probe)
    expect(runner.indexOf(executableCheck)).toBeLessThan(runner.indexOf(probe))
    expect(runner).toContain("runner_failure 'installed ACP adapter initialize probe failed'")
  })
})

describe('installed desktop client identity', () => {
  const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/linux.sh'), 'utf8')

  it('copies the WebDriver build to the installed desktop path before each phase', () => {
    expect(runner).toMatch(/^installed_desktop=\/usr\/bin\/muniment-desktop$/m)
    expect(runner).toContain('[[ -f $installed_desktop ]]')
    expect(runner).toContain('sudo install -m 0755 "$e2e_app_binary" "$installed_desktop"')
    expect(runner).toContain("runner_failure 'installed desktop path could not use the E2E build'")
    expect(runner.indexOf('sudo install -m 0755 "$e2e_app_binary" "$installed_desktop"'))
      .toBeLessThan(runner.indexOf('run_e2e "$raw/wdio-onboarding.log"'))
  })

  it('reads the shipped binary before that path changes', () => {
    expect(runner.indexOf('webdriver-release-guard.mjs absent'))
      .toBeLessThan(runner.indexOf('sudo install -m 0755 "$e2e_app_binary" "$installed_desktop"'))
  })

  it('fails the run when the installed copy differs', () => {
    expect(runner).toContain('cmp -s "$e2e_app_binary" "$installed_desktop"')
    expect(runner).toContain("runner_failure 'installed desktop path does not contain the E2E build'")
    expect(runner.indexOf('cmp -s "$e2e_app_binary" "$installed_desktop"'))
      .toBeGreaterThan(runner.indexOf('sudo install -m 0755 "$e2e_app_binary" "$installed_desktop"'))
  })
})

describe('folder dialog diagnostics', () => {
  it('records the searched title and visible windows after a timeout', async () => {
    const raw = temp()
    const title = '(Choose).*([Ff]older)'
    const timeoutError = Object.assign(new Error('dialog wait timed out'), { code: 124 })
    const execute = async (command, args) => {
      if (command === 'timeout') throw timeoutError
      if (args[0] === 'search') return { stdout: '101\n202\n' }
      if (args.at(-1) === '101') return { stdout: 'Muniment\n' }
      return { stdout: 'Choose a Folder\n' }
    }
    const { chooseFolder } = await import('./e2e/support/folder-dialog.mjs')
    await expect(chooseFolder('/tmp/home', 30, title, raw, execute)).rejects.toBe(timeoutError)
    expect(fs.readFileSync(path.join(raw, 'folder-picker-timeout.log'), 'utf8')).toBe([
      `searched title: ${title}`,
      'visible window titles:',
      '101: Muniment',
      '202: Choose a Folder',
      '',
    ].join('\n'))
  })
})

describe('Windows nightly release lookup', () => {
  const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/windows.ps1'), 'utf8')
  const lookup = runner.slice(runner.indexOf('  # Resolve all identity checks'), runner.indexOf('  $installer ='))

  it('uses the REST API with the injected token and forwards the response body unchanged', () => {
    expect(lookup).toContain('$release = (Invoke-WebRequest -UseBasicParsing -Headers @{ Accept = "application/vnd.github+json"; Authorization = "Bearer $($env:GH_TOKEN)" }')
    expect(lookup).toContain('-Uri "https://api.github.com/repos/$($env:GITHUB_REPOSITORY)/releases/tags/nightly").Content')
    expect(lookup).toContain('Add-Content -LiteralPath $installerLog -Value $release -NoNewline')
    expect(lookup).toContain('Invoke-NativeCommand "node" "test/e2e/support/asset-identity.mjs $sha windows" $installerLog "Windows artifact identity validation failed" $release')
    expect(lookup).not.toMatch(/ConvertFrom-Json|ConvertTo-Json|\.Trim\(/)
    expect(lookup).toContain('Invoke-WebRequest -UseBasicParsing -Headers @{ Accept = "application/octet-stream"; Authorization = "Bearer $($env:GH_TOKEN)" }')
    expect(lookup).toContain('-Uri "https://api.github.com/repos/$($env:GITHUB_REPOSITORY)/releases/assets/$assetId" -OutFile $msi')
  })

  it('throws lookup failures with the exception text and logs runner failures', () => {
    expect(lookup).toMatch(/try \{\s*\$release = \(Invoke-WebRequest[\s\S]+?\} catch \{\s*throw "nightly release lookup failed: \$\(\$_\.Exception\.Message\)"\s*\}/)
    expect(runner).toContain(String.raw`if ($installerLog) { Add-Content $installerLog "runner failed: $($_.Exception.Message -replace '\r?\n', ' ')" -ErrorAction SilentlyContinue }`)
  })

  it('does not depend on the GitHub CLI', () => {
    expect(runner).not.toMatch(/\bgh(?:\.exe)?\b/i)
  })
})

describe('Windows bounded process contract', { timeout: 30_000 }, () => {
  const runnerPath = path.join(root, 'test/e2e/runner/windows.ps1')
  const runner = fs.readFileSync(runnerPath, 'utf8')
  const helper = runner.slice(runner.indexOf('function Invoke-BoundedProcess'), runner.indexOf('function Resolve-NativeCommand'))

  it('Caches the process handle before the bounded wait.', () => {
    expect(helper).toMatch(/\$null = \$process\.Handle\s+\$timedOut = -not \$process\.WaitForExit\(\$TimeoutSeconds \* 1000\)/)
    expect(helper).toContain('$stopped = $process.WaitForExit(10000)')
    expect(helper).not.toContain('-Wait')
    expect(helper).toContain('if ($null -eq $exitCode) { throw "$File did not report an exit code" }')
    expect(helper).toContain('throw "$File failed with exit code $exitCode"')
    expect(helper).toMatch(/finally \{\s+\$process\.Dispose\(\)/)
  })

  it.skipIf(process.platform !== 'win32').each([0, 3010, 7, 1603, 3011, -1])(
    'Reports exit code %s and publishes the runner artifacts.', (exitCode) => {
      const directory = temp()
      const artifacts = path.join(directory, 'artifacts')
      const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', runnerPath], {
        encoding: 'utf8', timeout: 25_000,
        env: { ...process.env, TEMP: directory, TMP: directory, DCI_ARTIFACTS_DIR: artifacts,
          MUNIMENT_E2E_FINALIZER_TEST_MODE: '1', MUNIMENT_E2E_BOUNDED_PROCESS_TEST_EXIT_CODE: String(exitCode) },
      })
      const succeeds = exitCode === 0 || exitCode === 3010
      expect(result.status, result.stdout + result.stderr).toBe(succeeds ? 0 : 1)
      const log = fs.readFileSync(path.join(artifacts, 'installer.log'), 'utf8')
      expect(log).toContain('bounded stdout')
      expect(log).toContain('bounded stderr')
      expect(log).toContain(`cmd.exe exited with code ${exitCode}`)
      const failure = path.join(artifacts, 'runner-failure.txt')
      if (succeeds) {
        expect(fs.existsSync(failure)).toBe(false)
      } else {
        const message = `cmd.exe failed with exit code ${exitCode}`
        expect(result.stdout).toContain(message)
        expect(fs.readFileSync(failure, 'utf8')).toContain(`message: ${message}`)
      }
    },
  )

  it.skipIf(process.platform !== 'win32')('Stops a process that exceeds the timeout.', () => {
    const directory = temp()
    const script = path.join(directory, 'timeout.ps1')
    const child = path.join(directory, 'slow child.ps1')
    const log = path.join(directory, 'timeout.log')
    fs.writeFileSync(child, 'Start-Sleep -Seconds 60\n')
    fs.writeFileSync(script, `param([string]$Child, [string]$Log)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
${helper}
try {
  Invoke-BoundedProcess 'powershell.exe' "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File \`"$Child\`"" 1 $Log
  exit 0
} catch {
  Write-Output $_.Exception.Message
  exit 1
}
`)
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script, child, log], {
      encoding: 'utf8', timeout: 20_000,
    })
    expect(result.status, result.stdout + result.stderr).toBe(1)
    expect(result.stdout).toContain('powershell.exe timed out')
    expect(fs.readFileSync(log, 'utf8')).toMatch(/powershell\.exe exited with code -?\d+/)
  })
})

describe('Windows toolchain and failure evidence', { timeout: 30_000 }, () => {
  const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/windows.ps1'), 'utf8')

  it('Uses the local CLI and records each toolchain boundary.', () => {
    expect(runner).toContain('$startInfo.WorkingDirectory = (Get-Location).ProviderPath')
    expect(runner).toContain('Set-Location -LiteralPath $repoRoot')
    const ci = runner.indexOf('"ci --no-audit --no-fund"')
    const first = runner.indexOf('Write-ToolchainState "after-npm-ci"')
    const tests = runner.indexOf('"vitest run --root .')
    const second = runner.indexOf('Write-ToolchainState "after-contract-tests"')
    const install = runner.lastIndexOf('\n  Install-Product')
    const last = runner.indexOf('Write-ToolchainState "before-e2e-build"')
    const build = runner.indexOf('Invoke-NativeCommand "node" "`"$tauriCli`" build')
    expect([ci, first, tests, second, install, last, build].every((value) => value >= 0)).toBe(true)
    expect([ci, first, tests, second, install, last, build]).toEqual([ci, first, tests, second, install, last, build].sort((a, b) => a - b))
    expect(runner).toContain('Test-Path -LiteralPath $tauriCli -PathType Leaf')
    expect(runner).toContain('exec --offline --call')
    expect(runner).not.toContain('exec --offline -- node')
    expect(runner).not.toContain('"run tauri -- build')
    expect(runner).toContain('$null "diagnostic summary failed" $summaryInput $null $false')
    const tail = runner.indexOf('../support/installer-log-tail.mjs')
    expect(tail).toBeGreaterThan(0)
    expect(tail).toBeLessThan(runner.indexOf('\n  Stop-Transcript '))
  })

  it('Runs the npm probe with a fresh prefix and cache without package inference.', () => {
    const directory = temp()
    const repo = path.join(directory, 'repo [fixture] & spaces')
    const support = path.join(repo, 'test/e2e/support')
    const prefix = path.join(directory, 'fresh prefix')
    const cache = path.join(directory, 'fresh cache')
    const bin = path.join(repo, 'node_modules/.bin')
    for (const folder of [support, prefix, cache, bin]) fs.mkdirSync(folder, { recursive: true })
    fs.writeFileSync(path.join(repo, 'package.json'), '{"private":true}')
    for (const name of ['windows-toolchain.mjs', 'redact-text.mjs', 'failure-summary.mjs']) {
      fs.copyFileSync(path.join(root, 'test/e2e/support', name), path.join(support, name))
    }
    const env = Object.fromEntries(Object.entries(process.env)
      .filter(([key]) => !/^npm_config_(prefix|cache)$/i.test(key)))
    env.npm_config_prefix = prefix
    env.npm_config_cache = cache
    const options = { cwd: repo, env, encoding: 'utf8', timeout: 20_000 }
    const node = spawnSync('node', ['--version'], options)
    expect(node.status, node.stderr).toBe(0)
    expect(fs.readdirSync(prefix)).toEqual([])
    expect(fs.readdirSync(cache)).toEqual([])

    let result
    if (process.platform === 'win32') {
      const script = path.join(repo, 'test/e2e/runner/probe.ps1')
      fs.mkdirSync(path.dirname(script), { recursive: true })
      const helpers = runner.slice(runner.indexOf('function Resolve-NativeCommand'), runner.indexOf('function Get-UninstallEntries'))
      fs.writeFileSync(script, `param([string]$repoRoot, [string]$installerLog)
$ErrorActionPreference = 'Stop'
${helpers}
Set-Location -LiteralPath $repoRoot
[Environment]::CurrentDirectory = $env:SystemRoot
Write-ToolchainState 'before-e2e-build'
`)
      result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script,
        repo, path.join(directory, 'installer.log')], options)
    } else {
      // Run the runner's shell command on Linux without PowerShell.
      const call = runner.match(/exec --offline --call `"([^"\r\n]+)`"/)
      expect(call).not.toBeNull()
      result = spawnSync('npm', ['exec', '--offline', '--call', call[1].replace('$Phase', 'before-e2e-build')], options)
    }
    expect(result.status, result.stdout + result.stderr).toBe(0)
    expect(result.stdout).toContain('Tauri toolchain before-e2e-build')
    expect(result.stdout).toContain(`cwd=${repo}`)
    expect(result.stdout).toContain('npm_path_has_local_bin=true')
    expect(result.stdout).toContain('The local Tauri CLI entry is missing.')
    expect(result.stdout).toContain('The local tauri.cmd shim is missing.')
    expect(result.stdout + result.stderr).not.toContain('ENOTCACHED')
    expect(fs.existsSync(path.join(cache, '_npx'))).toBe(false)
  })

  it.each(['missing shim', 'missing CLI', 'missing PATH', 'missing PATHEXT', 'present'])('Names the %s toolchain state.', (state) => {
    const directory = temp()
    const bin = path.join(directory, 'node_modules', '.bin')
    const cli = path.join(directory, 'node_modules', '@tauri-apps', 'cli', 'tauri.js')
    fs.mkdirSync(bin, { recursive: true })
    fs.mkdirSync(path.dirname(cli), { recursive: true })
    if (state !== 'missing shim') fs.writeFileSync(path.join(bin, 'tauri.cmd'), '@echo fixture\r\n')
    if (state !== 'missing CLI') fs.writeFileSync(cli, '')
    const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !/^path$/i.test(key)))
    env.PATH = state === 'missing PATH' ? '' : bin
    env.PATHEXT = state === 'missing PATHEXT' ? '.EXE' : '.EXE;.CMD'
    const result = runNode('test/e2e/support/windows-toolchain.mjs', [directory, 'fixture'], { env })
    expect(result.status, result.stderr).toBe(0)
    expect(result.stdout.includes('The local tauri.cmd shim is missing.')).toBe(state === 'missing shim')
    expect(result.stdout.includes('The local Tauri CLI entry is missing.')).toBe(state === 'missing CLI')
    expect(result.stdout.includes('PATH omits the local')).toBe(state === 'missing PATH')
    expect(result.stdout.includes('PATHEXT omits .CMD')).toBe(state === 'missing PATHEXT')
  })

  it('Runs the local CLI without a command PATH or .CMD lookup.', () => {
    const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !/^path$/i.test(key)))
    const result = runNode('node_modules/@tauri-apps/cli/tauri.js', ['--version'], {
      env: { ...env, PATH: '', PATHEXT: '.EXE' }, timeout: 15_000,
    })
    expect(result.status, result.stderr).toBe(0)
    expect(result.stdout).toMatch(/^tauri-cli \d+\./)
  })

  it.each([0, 1, 40, 41, 80])('Shows the last 40 installer lines from %s lines.', (count) => {
    const log = path.join(temp(), 'installer.log')
    const lines = Array.from({ length: count }, (_, index) => `installer line ${index}`)
    fs.writeFileSync(log, lines.join('\r\n') + (count ? '\r\n' : ''))
    const result = runNode('test/e2e/support/installer-log-tail.mjs', [log])
    expect(result.status, result.stderr).toBe(0)
    expect(result.stdout).toBe(`dci: installer.log last 40 lines\n${lines.slice(-40).join('\n')}\n`)
  })

  it('Redacts a secret across the installer line cutoff.', () => {
    const log = path.join(temp(), 'installer.log')
    const secret = 'first secret line\nsecond secret line'
    fs.writeFileSync(log, secret + '\n' + 'safe line\n'.repeat(39))
    const result = runNode('test/e2e/support/installer-log-tail.mjs', [log], {
      env: { ...process.env, MUNIMENT_E2E_PASSWORD: secret },
    })
    expect(result.status).toBe(0)
    expect(result.stdout).toContain('[REDACTED]')
    expect(result.stdout).not.toContain('secret line')
  })

  it.skipIf(process.platform !== 'win32')('Uses the PowerShell location when the process current directory differs.', () => {
    const directory = temp()
    const script = path.join(directory, 'cwd [fixture].ps1')
    const probe = path.join(directory, 'cwd [fixture].mjs')
    const helpers = runner.slice(runner.indexOf('function Resolve-NativeCommand'), runner.indexOf('function Write-ToolchainState'))
    fs.writeFileSync(probe, 'console.log(process.cwd())')
    fs.writeFileSync(script, `param([string]$Root, [string]$Probe, [string]$Log)
$ErrorActionPreference = 'Stop'
${helpers}
Set-Location -LiteralPath $Root
[Environment]::CurrentDirectory = $env:SystemRoot
Invoke-NativeCommand 'node' "\`"$Probe\`"" $Log 'cwd probe failed'
`)
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script,
      directory, probe, path.join(directory, 'cwd.log')], { encoding: 'utf8', timeout: 20_000 })
    expect(result.status, result.stdout + result.stderr).toBe(0)
    expect(result.stdout.trim()).toBe(directory)
  })

  it.skipIf(process.platform !== 'win32')('Publishes the installer tail in the failure transcript.', () => {
    const directory = temp()
    const script = path.join(directory, 'failure.mjs')
    const artifacts = path.join(directory, 'artifacts')
    fs.writeFileSync(script, `console.log('progress\\n'.repeat(60)); console.error('error[E0123]: fixture-secret build failed'); process.exitCode = 7`)
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(root, 'test/e2e/runner/windows.ps1')], {
      encoding: 'utf8', timeout: 25_000,
      env: { ...process.env, TEMP: directory, TMP: directory, DCI_ARTIFACTS_DIR: artifacts,
        MUNIMENT_E2E_NATIVE_COMMAND_TEST_SCRIPT: script, MUNIMENT_E2E_PASSWORD: 'fixture-secret' },
    })
    expect(result.status, result.stdout + result.stderr).toBe(1)
    const log = fs.readFileSync(path.join(artifacts, 'installer.log'), 'utf8').replaceAll('\r\n', '\n')
    expect(log.match(/^progress$/gm)).toHaveLength(60)
    expect(log).toContain('error[E0123]: [REDACTED] build failed')
    expect(log.trimEnd().split('\n').slice(-40).join('\n')).toContain('error[E0123]: [REDACTED] build failed')
    expect(log).not.toContain('fixture-secret')
    const transcript = fs.readFileSync(path.join(artifacts, 'runner-transcript.log'), 'utf8')
    const tail = transcript.slice(transcript.indexOf('dci: installer.log last 40 lines'))
    expect(tail).toContain('dci: installer.log last 40 lines')
    expect(tail).toContain('error[E0123]: [REDACTED] build failed')
    expect(tail).not.toContain('fixture-secret')
    expect(result.stdout).toContain('dci: installer.log last 40 lines')
  })
})

describe('Windows native command contract', { timeout: 30_000 }, () => { // A PowerShell spawn costs about 3.5 seconds, and the slowest observed test took 6993ms.
  it('routes native commands through the process helpers', () => {
    const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/windows.ps1'), 'utf8')
    expect(runner).not.toMatch(/&\s+(?!\$Action\b)[^;\r\n|}]*\s\*>>/)
    expect(runner).not.toMatch(/&\s+(?:npm(?:\.cmd)?|node(?:\.exe)?|gh(?:\.exe)?|cargo(?:\.exe)?|git(?:\.exe)?)(?=\s|$)/im)
  })

  it.skipIf(process.platform !== 'win32').each([
    ['0', 0, 'stderr'],
    ['7', 1, 'stderr'],
    ['0', 0, 'stdout'],
    ['7', 1, 'stdout'],
    ['0', 0, 'both'],
    ['7', 1, 'both'],
  ])('gates exit code %s with expected status %s and %s output', (exitCode, expectedStatus, output) => {
    const directory = temp()
    const artifacts = path.join(directory, 'artifacts')
    const runner = path.join(root, 'test/e2e/runner/windows.ps1')
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', runner], {
      encoding: 'utf8',
      env: { ...process.env, TEMP: directory, TMP: directory, DCI_ARTIFACTS_DIR: artifacts,
        MUNIMENT_E2E_NATIVE_COMMAND_TEST_EXIT_CODE: exitCode, MUNIMENT_E2E_NATIVE_COMMAND_TEST_OUTPUT: output },
    })
    expect(result.status).toBe(expectedStatus)
    const messages = output === 'stderr' ? ['native warning'] : ['fatal: installer rejected package signature']
    if (output === 'both') messages.push('native warning')
    const log = fs.readFileSync(path.join(artifacts, 'installer.log'), 'utf8')
    for (const message of messages) expect(log).toContain(message)
    if (expectedStatus) {
      const cause = fs.readFileSync(path.join(artifacts, 'runner-failure.txt'), 'utf8')
      expect(cause).toContain(`native command test failed (exit code 7): ${messages.join('\n')}`)
    } else {
      expect(fs.existsSync(path.join(artifacts, 'runner-failure.txt'))).toBe(false)
      if (output !== 'stderr') expect(result.stdout).toContain(messages[0])
    }
  })

  it.skipIf(process.platform !== 'win32').each(['long-stdout', 'long-stderr'])(
    'keeps both streams within the JUnit cap with %s progress', (output) => {
      const directory = temp()
      const artifacts = path.join(directory, 'artifacts')
      const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(root, 'test/e2e/runner/windows.ps1')], {
        encoding: 'utf8',
        env: { ...process.env, TEMP: directory, TMP: directory, DCI_ARTIFACTS_DIR: artifacts,
          MUNIMENT_E2E_NATIVE_COMMAND_TEST_EXIT_CODE: '7', MUNIMENT_E2E_NATIVE_COMMAND_TEST_OUTPUT: output },
      })
      expect(result.status).toBe(1)
      const fatal = 'fatal: installer rejected package signature'
      const log = fs.readFileSync(path.join(artifacts, 'installer.log'), 'utf8')
      expect(log).toContain('p'.repeat(1800))
      expect(log).toContain(fatal)
      const cause = fs.readFileSync(path.join(artifacts, 'runner-failure.txt'), 'utf8')
      expect(cause.length).toBeLessThanOrEqual(1000)
      const message = Array.from(cause.replace(/\s+/gu, ' ').trim()).slice(0, 1000).join('')
      expect(message).toContain('native command test failed (exit code 7)')
      expect(message).toContain(fatal)
      expect(message).toContain('p'.repeat(100))
      expect(message).toContain(' ... ')
    },
  )

  it.skipIf(process.platform !== 'win32')('fails when PowerShell cannot invoke the command', () => {
    const directory = temp()
    const artifacts = path.join(directory, 'artifacts')
    const runner = path.join(root, 'test/e2e/runner/windows.ps1')
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', runner], {
      encoding: 'utf8',
      env: { ...process.env, TEMP: directory, TMP: directory, DCI_ARTIFACTS_DIR: artifacts, MUNIMENT_E2E_NATIVE_COMMAND_TEST_INVOCATION_ERROR: '1' },
    })
    expect(result.status).toBe(1)
    expect(fs.readFileSync(path.join(artifacts, 'installer.log'), 'utf8')).toContain('muniment-command-that-does-not-exist')
    expect(fs.readFileSync(path.join(artifacts, 'runner-failure.txt'), 'utf8'))
      .toContain('native command test failed: could not resolve muniment-command-that-does-not-exist')
  })

  it.skipIf(process.platform !== 'win32')('redacts the terminating cause before publication', () => {
    const directory = temp()
    const artifacts = path.join(directory, 'artifacts')
    const secret = 'muniment-command-that-does-not-exist'
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(root, 'test/e2e/runner/windows.ps1')], {
      encoding: 'utf8',
      env: { ...process.env, TEMP: directory, TMP: directory, DCI_ARTIFACTS_DIR: artifacts,
        MUNIMENT_E2E_NATIVE_COMMAND_TEST_INVOCATION_ERROR: '1', MUNIMENT_E2E_PASSWORD: secret },
    })
    expect(result.status).toBe(1)
    const cause = fs.readFileSync(path.join(artifacts, 'runner-failure.txt'), 'utf8')
    expect(cause).toContain('could not resolve [REDACTED]')
    expect(cause).not.toContain(secret)
  })

  it.skipIf(process.platform !== 'win32').each([false, true])('Resolves the first PATH match instead of the working directory with duplicate commands %s.', (duplicates) => {
    const directory = temp()
    const artifacts = path.join(directory, 'artifacts')
    const decoy = path.join(directory, 'npm.cmd')
    const runner = path.join(root, 'test/e2e/runner/windows.ps1')
    fs.writeFileSync(decoy, '@echo decoy\r\n')
    const env = { ...process.env, TEMP: directory, TMP: directory, DCI_ARTIFACTS_DIR: artifacts, MUNIMENT_E2E_NATIVE_COMMAND_TEST_RESOLUTION: '1' }
    const first = path.join(directory, 'first')
    const second = path.join(directory, 'second')
    if (duplicates) {
      for (const entry of [first, second]) {
        fs.mkdirSync(entry)
        fs.writeFileSync(path.join(entry, 'npm.cmd'), '@echo fixture\r\n')
      }
      for (const key of Object.keys(env).filter((key) => key.toLowerCase() === 'path')) {
        env[key] = [first, second, env[key]].join(path.delimiter)
      }
    }
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', runner], {
      cwd: directory,
      encoding: 'utf8',
      env,
    })
    expect(result.status).toBe(0)
    const resolved = fs.readFileSync(path.join(artifacts, 'installer.log'), 'utf8').trim()
    expect(path.isAbsolute(resolved)).toBe(true)
    expect(path.resolve(resolved).toLowerCase()).not.toBe(path.resolve(decoy).toLowerCase())
    if (duplicates) expect(resolved).toBe(path.join(first, 'npm.cmd'))
  })
})

describe('onboarding Home path assertion', () => {
  it('accepts a matching DOM path when rendered text is empty', async () => {
    const { homePathMatches } = await import('./e2e/support/home-path.mjs')
    const location = {
      getProperty: async () => '/tmp/isolated-home',
      getText: async () => '',
    }

    expect(await homePathMatches(location, '/tmp/isolated-home')).toBe(true)
  })

  it('rejects a wrong DOM path', async () => {
    const { homePathMatches } = await import('./e2e/support/home-path.mjs')
    const location = {
      getProperty: async () => '/tmp/wrong-home',
      getText: async () => '/tmp/isolated-home',
    }

    expect(await homePathMatches(location, '/tmp/isolated-home')).toBe(false)
  })
})

describe('Linux E2E shared-library contract', () => {
  it('probes the installed runtime without a library path override', () => {
    const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/linux.sh'), 'utf8')
    expect(runner).toContain('unset LD_LIBRARY_PATH\nruntime_version=$(/usr/lib/muniment/muniment-runtime --version)')
    expect(runner).not.toContain('export LD_LIBRARY_PATH=')
  })
})

describe('hosted sign-in teardown contract', () => {
  const spec = fs.readFileSync(path.join(root, 'test/e2e/specs/real-sign-in.spec.js'), 'utf8')

  it('does not let hosted teardown replace sign-in results', () => {
    expect(spec).toContain("if (signInBrowser && process.platform !== 'linux') await signInBrowser.deleteSession()")
    expect(spec).toMatch(/catch \(error\) \{\s*console\.error\('Failed to delete hosted sign-in session\.', error\)\s*\} finally \{\s*if \(authDriver\) authDriver\.kill\(\)\s*\}/)
  })
})

describe('installed local-mode chat contract', () => {
  const config = fs.readFileSync(path.join(root, 'test/e2e/wdio.conf.js'), 'utf8')
  const spec = fs.readFileSync(path.join(root, 'test/e2e/specs/local-mode-chat.spec.js'), 'utf8')
  const localProvider = fs.readFileSync(path.join(root, 'test/e2e/support/local-provider.mjs'), 'utf8')
  const firstRun = fs.readFileSync(path.join(root, 'test/e2e/support/first-run.mjs'), 'utf8')

  it('runs local mode first without bailing after sign-in failures', () => {
    expect(config.indexOf("'./specs/local-mode-chat.spec.js'")).toBeLessThan(config.indexOf("'./specs/real-sign-in.spec.js'"))
    expect(config).toContain('bail: 0')
    expect(config).toContain("path.basename(specs[0], '.spec.js')")
  })

  it('stores the local provider and waits for the first reply', () => {
    expect(localProvider).toContain('export const OLLAMA_BASE_URL')
    expect(localProvider).toContain('10.1.10.105')
    expect(spec).toContain("import { OLLAMA_BASE_URL } from '../support/local-provider.mjs'")
    expect(spec).not.toContain('MUNIMENT_E2E_PROVIDER_KEY')
    expect(spec).toContain("import { expandSidebar, openFirstRunModelSettings } from '../support/first-run.mjs'")
    expect(spec).toContain('await openFirstRunModelSettings()')
    expect(firstRun).toContain("panel.$('button=Open model settings')")
    expect(firstRun).toContain("$('#onboarding-model-panel')")
    expect(spec).toContain("$('button=Save Ollama server')")
    expect(spec).toContain("response.$('.response-prose.streaming')")
    expect(spec).not.toContain('browser.tauri.mock')
  })
})

describe('The installed specs use first-run selectors.', () => {
  it.each(['local-mode-chat', 'onboarding', 'real-sign-in'])('The %s spec uses the first-run model panel.', (name) => {
    const spec = fs.readFileSync(path.join(root, `test/e2e/specs/${name}.spec.js`), 'utf8')
    expect(spec).not.toContain('#local-account-title')
    expect(spec).not.toContain('input[name="provider"]')
    expect(spec).toContain('await openFirstRunModelSettings()')
    expect(spec).toContain('[data-testid="local-mode"]')
  })
})

describe('Windows build MSI diagnostics', { timeout: 30_000 }, () => {
  const script = fs.readFileSync(path.join(root, 'test/windows-installers.ps1'), 'utf8')
  const powershell = process.platform === 'win32' ? 'powershell.exe' : 'pwsh'
  const hasPowerShell = process.platform === 'win32' || spawnSync(powershell, ['-NoProfile', '-Command', 'exit 0'], { timeout: 15_000 }).status === 0
  const registration = fs.readFileSync(path.join(root, 'test/windows-msi-registration.ps1'), 'utf8')
  const helpers = registration + '\n' + script.slice(script.indexOf('function Write-MsiScopeLog'), script.indexOf('\n$machineKey ='))
  const invoke = (body, args = []) => {
    const file = path.join(temp(), 'diagnostics.ps1')
    // Replace COM identity and release calls. The fixture runs the registration helper and its property getters.
    const fixtureHelpers = helpers.replaceAll(
      '[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject(', '[void](Release-FixtureComObject ',
    ).replaceAll('[Runtime.InteropServices.Marshal]::IsComObject(', '(Test-FixtureComObject ')
    fs.writeFileSync(file, `$ErrorActionPreference = "Stop"\n[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)\n${fixtureHelpers}\n${body}\n`)
    return spawnSync(powershell, ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', file, ...args], {
      encoding: 'utf8', timeout: 15_000,
    })
  }

  const comFixture = `
$script:fixtureInstalled = $true
$script:fixtureContext = 2
$script:fixtureSid = 'S-1-5-21-123'
$script:fixtureCount = 1
$script:fixtureState = 5
$script:fixtureProductLeftover = $false
$script:fixtureRecord = $true
$script:released = @()
$script:getters = @()
$script:queries = @()
$script:closed = 0
function Test-FixtureComObject($Object) { return $null -ne $Object.Tag }
function Release-FixtureComObject($Object) { $script:released += $Object.Tag; return 0 }
$script:comReflector = [PSCustomObject]@{}
$script:comReflector | Add-Member ScriptMethod InvokeMember {
  param($Name, $Flags, $Binder, $Target, $Arguments)
  if ([Reflection.BindingFlags]$Flags -ne [Reflection.BindingFlags]::GetProperty -or $null -ne $Binder -or
      $Target.Tag -ne 'product') { throw 'Invalid COM getter arguments.' }
  if ($Name -eq 'InstallProperty') {
    if ($Arguments.Count -ne 1 -or $Arguments[0] -ne 'State') { throw 'Invalid MSI state arguments.' }
  } elseif ($null -ne $Arguments) { throw 'Invalid COM property arguments.' }
  $script:getters += $Name
  switch ($Name) {
    'ProductCode' { return '{12345678-1234-ABCD-EF12-34567890ABCD}' }
    'Context' { return $script:fixtureContext }
    'UserSid' { return $script:fixtureSid }
    'InstallProperty' { return $script:fixtureState }
    default { throw "Unexpected COM property: $Name" }
  }
}
# The product exposes properties through IDispatch, not PowerShell property lookup.
$script:comProduct = [PSCustomObject]@{ Tag = 'product' }
$script:comProduct | Add-Member ScriptMethod GetType { return $script:comReflector } -Force
$script:comRecord = [PSCustomObject]@{ Tag = 'record' }
$script:comRecord | Add-Member ScriptMethod StringData {
  param($Index)
  if ($Index -ne 1) { throw 'Invalid MSI record field.' }
  return '{12345678-1234-ABCD-EF12-34567890ABCD}'
}
$script:comView = [PSCustomObject]@{ Tag = 'view' }
$script:comView | Add-Member ScriptMethod Execute { return '' }
$script:comView | Add-Member ScriptMethod Close { $script:closed++; return '' }
$script:comView | Add-Member ScriptMethod Fetch { if ($script:fixtureRecord) { return $script:comRecord } }
$script:comDatabase = [PSCustomObject]@{ Tag = 'database' }
$script:comDatabase | Add-Member ScriptMethod OpenView {
  param($Query)
  $script:queries += $Query
  return $script:comView
}
$script:comInstaller = [PSCustomObject]@{ Tag = 'installer' }
$script:comInstaller | Add-Member ScriptMethod OpenDatabase {
  param($Package, $Mode)
  if ($Package -ne $argsPath -or $Mode -ne 0) { throw 'Invalid MSI database arguments.' }
  return $script:comDatabase
}
$script:comInstaller | Add-Member ScriptMethod ProductsEx {
  param($Code, $Sid, $Context)
  if ($Code -ne '{12345678-1234-ABCD-EF12-34567890ABCD}' -or $Sid -ne 'S-1-5-21-123' -or $Context -ne 7) {
    throw 'Invalid MSI registration scope.'
  }
  if (-not $script:fixtureInstalled) { Write-Host 'MSI product cleanup check.' }
  if ($script:fixtureInstalled -or $script:fixtureProductLeftover) { @($script:comProduct) * $script:fixtureCount }
}
function New-Object($ComObject) {
  if ($ComObject -ne 'WindowsInstaller.Installer') { throw 'Unexpected COM object.' }
  return $script:comInstaller
}
$argsPath = $args[0]
`

  it('Reads every built MSI before any install starts.', () => {
    expect(script).toContain('New-Object -ComObject WindowsInstaller.Installer')
    expect(script).toContain('$installer.OpenDatabase((Resolve-Path -LiteralPath $Package).Path, 0)')
    expect(script).toContain('SELECT ``Value`` FROM ``Property`` WHERE ``Property``')
    expect(script).toContain('if ($null -eq $record) { "absent" } else { $record.StringData(1) }')
    expect(script).toContain('$database.SummaryInformation(0)')
    expect(script).toContain('([int]$summary.Property(15) -band 8)')
    expect(script).toContain('msi properties $(Split-Path -Leaf $Package): ALLUSERS=$($values.ALLUSERS) MSIINSTALLPERUSER=$($values.MSIINSTALLPERUSER) InstallScope=$scope')
    expect(script).toContain('@($regularMsi[0].FullName, $machineMsi[0].FullName, $upgradeBaseMsi)')
    expect(script.indexOf('  Write-MsiProperties $package')).toBeLessThan(script.indexOf('Invoke-Msi "/i"'))
  })

  it('Completes both MSI lifecycles before NSIS can retain the shared HKCU key.', () => {
    const steps = [
      'Invoke-Msi "/x" $machineMsi.FullName',
      'if (Test-Path $machineKey) { throw "Machine registration remains after MSI uninstall" }',
      'throw "Machine uninstall registration remains after MSI uninstall"',
      'Remove-Item $upgradeBaseMsi -Force',
      '$userRuntime = Join-Path $env:LOCALAPPDATA "muniment\\muniment-runtime.exe"',
      'Invoke-Msi "/i" $regularMsi[0].FullName',
      'Assert-MsiProductContext $userRegistrations $sessionSid',
      'Invoke-Msi "/x" $regularMsi[0].FullName',
      'Assert-PerUserMsiRegistration $registryRegistration $env:LOCALAPPDATA -Absent',
      'throw "The MSI product registration remains after the per-user uninstall."',
      'if (Test-Path $userRuntime) { throw "The runtime remains after the per-user MSI uninstall." }',
      'if (Test-Path $userKey) { throw "The application registration remains after the per-user MSI uninstall." }',
      '$nsisProcess = Start-Process $nsis.FullName',
      '$nsisUninstall = Start-Process $nsisUninstaller',
      'if (Test-Path $userRuntime) { throw "NSIS runtime remains after uninstall at $userRuntime" }',
    ]
    let previous = -1
    for (const step of steps) {
      const position = script.indexOf(step)
      expect(position, step).toBeGreaterThan(previous)
      previous = position
    }
  })

  it('Prints scope evidence before install errors and registration assertions exit.', () => {
    expect(script).toMatch(/Invoke-Msi "\/i" \$regularMsi\[0\]\.FullName "Silent regular MSI install" \$userMsiLog\s*\} finally \{\s*Write-MsiScopeLog \$userMsiLog/)
    expect(script).toContain('[Security.Principal.WindowsIdentity]::GetCurrent().User.Value')
    expect(script).toContain('Get-UserRegistrations "Registry::HKEY_USERS\\$sessionSid"')
    expect(script).toContain('HKU\\$sessionSid=$($hku.Count) hkcu=$($hkcu.Count) hklm=$($hklm.Count)')
    expect(registration).toContain('"Registry::HKEY_USERS\\$Sid"')
    expect(registration).toContain('hkcu=$(@($uninstall[\'HKCU:\']).Count) HKU\\$sid=$(@($uninstall["Registry::HKEY_USERS\\$sid"]).Count) hklm=$(@($uninstall[\'HKLM:\']).Count)')
    expect(script.indexOf('Write-MsiScopeLog $userMsiLog')).toBeLessThan(script.indexOf('Assert-MsiProductContext $userRegistrations $sessionSid'))
    expect(script).toContain('@(Get-MsiRegistrations $regularMsi[0].FullName $sessionSid)')
    expect(registration).toContain('$installer.ProductsEx($record.StringData(1), $UserSid, 7)')
    expect(registration).toContain('The MSI must register one installed, unmanaged product for the current user and none for the machine.')
    expect(script).toContain('& (Join-Path $PSScriptRoot "windows-msi-registration.tests.ps1")')
    expect(script).toContain('The per-user MSI must register its LocalAppData install path under HKCU.')
    expect(script).toContain('The per-user MSI wrote application registration under HKLM.')
    expect(script).toContain('Get-PerUserMsiRegistration $userProductCode $sessionSid')
    expect(script).toContain('Write-PerUserMsiRegistration $registryRegistration "installed"')
    expect(script.indexOf('Write-MsiScopeLog $userMsiLog')).toBeLessThan(script.indexOf('Assert-PerUserMsiRegistration $registryRegistration'))
    expect(script).toContain('Assert-PerUserMsiRegistration $registryRegistration $env:LOCALAPPDATA -Absent')
    expect(script).not.toContain('requires hkcu=1 hklm=0')
    expect(script).toContain('if (Test-Path "HKCU:\\Software\\Muniment\\muniment")')
    expect(script).toContain('throw "Per-machine MSI wrote application registration under HKCU"')
    for (const workflow of ['ci.yml', 'nightly.yml']) {
      expect(fs.readFileSync(path.join(root, '.github/workflows', workflow), 'utf8')).toContain('-File test/windows-installers.ps1')
    }
  })

  it('Checks machine scope and regular MSI cleanup before NSIS starts.', () => {
    const machineCleanup = script.indexOf('Remove-Item $upgradeBaseMsi -Force')
    const regularInstall = script.indexOf('$userProductCode =')
    const regularCleanup = script.indexOf('if (Test-Path $userKey) { throw "The application registration remains after the per-user MSI uninstall." }')
    const nsisInstall = script.indexOf('$nsisProcess = Start-Process')
    expect(machineCleanup).toBeGreaterThan(0)
    expect(regularInstall).toBeGreaterThan(machineCleanup)
    expect(regularCleanup).toBeGreaterThan(regularInstall)
    expect(nsisInstall).toBeGreaterThan(regularCleanup)
  })

  it.skipIf(!hasPowerShell)('Prints scope changes and the installing user from a Unicode log.', () => {
    const log = path.join(temp(), 'install [fixture].log')
    const lines = [
      "MSI (s) (00:00): PROPERTY CHANGE: Adding ALLUSERS property. Its value is '1'.",
      "MSI (s) (00:00): PROPERTY CHANGE: Modifying MSIINSTALLPERUSER property. Its current value is '1'. Its new value: '0'.",
      'MSI (s) (00:00): PROPERTY CHANGE: Deleting ALLUSERS property. Its current value is 1.',
      "MSI (s) (00:00): PROPERTY CHANGE: Adding UserSID property. Its value is 'S-1-5-21-123'.",
      'Property(S): UserSID = S-1-5-21-123',
      'Property(C): LogonUser = Jos\u00e9',
      'Property(S): ALLUSERS = 1',
      "MSI (s): PROPERTY CHANGE: Adding MsiRunningElevated property. Its value is '1'.",
      'Property(S): MsiRunningElevated = 1',
    ]
    fs.writeFileSync(log, '\uFEFF' + [...lines,
      'PROPERTY CHANGE: Adding NOTALLUSERS property. Its value is 1.',
      'PROPERTY CHANGE: Adding Other property. Its value is ALLUSERS.',
      'Property(S): Unrelated = private',
    ].join('\r\n'), 'utf16le')
    const result = invoke('Write-MsiScopeLog $args[0]', [log])
    expect(result.status, result.stdout + result.stderr).toBe(0)
    for (const line of lines) expect(result.stdout).toContain(line)
    expect(result.stdout).not.toMatch(/NOTALLUSERS|Other property|Unrelated|private/)
  })

  it.skipIf(!hasPowerShell).each(['missing', 'empty'])('Names the %s log state.', (state) => {
    const log = path.join(temp(), 'install.log')
    if (state === 'empty') fs.writeFileSync(log, '')
    const result = invoke('Write-MsiScopeLog $args[0]', [log])
    expect(result.status, result.stderr).toBe(0)
    if (state === 'missing') expect(result.stdout).toContain('The per-user MSI verbose log is absent.')
    else for (const name of ['ALLUSERS', 'MSIINSTALLPERUSER', 'UserSID', 'LogonUser', 'MsiRunningElevated']) {
      expect(result.stdout).toContain(`The per-user MSI log has no ${name} lines.`)
    }
  })

  it.skipIf(!hasPowerShell).each([
    [1, 0, 1, 0, 2, 1], [0, 1, 0, 0, 2, 1], [0, 0, 0, 0, 2, 0],
    [2, 0, 2, 0, 2, 2], [1, 1, 1, 0, 2, 2], [1, 0, 0, 1603, 2, 1],
    [0, 1, 0, 0, 4, 1], [1, 0, 1, 0, 4, 1],
    [0, 0, 0, 0, 2, 1], [2, 0, 2, 0, 2, 1], [1, 1, 1, 0, 2, 1],
    ...['userProduct', 'userData', 'managed', 'machineProduct', 'leftover', 'application-leftover', 'product-leftover'].map((invalid) => [0, 1, 0, 0, 2, 1, invalid]),
  ])('Prints counts for HKCU=%s HKLM=%s HKU=%s, install code %s, context %s, and registrations %s.', (hkcu, hklm, hku, code, context, count, invalid = '') => {
    const msi = path.join(temp(), 'fixture.msi')
    fs.writeFileSync(msi, '')
    const runtime = path.join(path.dirname(msi), 'muniment', 'muniment-runtime.exe')
    fs.mkdirSync(path.dirname(runtime))
    fs.writeFileSync(runtime, '')
    // Replace the Windows identity boundary while the fixture runs both per-user installer cycles.
    const sequence = script.slice(script.indexOf('\n', script.indexOf('Remove-Item $upgradeBaseMsi -Force'))).replace(
      '[Security.Principal.WindowsIdentity]::GetCurrent().User.Value', '"S-1-5-21-123"',
    )
    const result = invoke(`
${comFixture}
$regularMsi = @([PSCustomObject]@{ FullName = $args[0] })
$script:fixtureContext = ${context}
$script:fixtureCount = ${count}
$nsis = @([PSCustomObject]@{ FullName = 'fixture-setup.exe' })
$userRuntime = $args[1]
$env:LOCALAPPDATA = Split-Path (Split-Path $userRuntime)
$script:applicationValues = @{}
$machineKey = "HKLM:\\Software\\Muniment\\muniment"
function Test-Path($Path, $LiteralPath) {
  if ($Path -eq $machineKey) { return $false }
  if ($Path -eq "HKCU:\\Software\\Muniment\\muniment") {
    if (-not $script:fixtureInstalled) { Write-Host 'MSI application cleanup check.' }
    return $script:applicationValues.Count -ne 0
  }
  if ($LiteralPath) { return Microsoft.PowerShell.Management\\Test-Path -LiteralPath $LiteralPath }
  return Microsoft.PowerShell.Management\\Test-Path $Path
}
function Get-ItemPropertyValue($Path, $Name) { return $script:applicationValues[$Name] }
function Get-UserRegistrations($Hive) {
  if ($Hive -eq "HKCU:") { @("fixture") * ${hkcu} } else { @("fixture") * ${hku} }
}
function Get-MunimentRegistrations { @('fixture') * ${hklm} }
function Get-MsiProductCode { return '{12345678-1234-ABCD-EF12-34567890ABCD}' }
$script:fixtureInstalled = $false
function Test-MsiUserLocation($Location, $LocalAppData) { return $Location -eq 'fixture-location' }
function Get-PerUserMsiRegistration($ProductCode, $Sid) {
  $entries = @()
  if ($script:fixtureInstalled) { $entries = @(@{ path = 'fixture-uninstall'; InstallLocation = 'fixture-location' }) }
  return @{
    ProductCode = $ProductCode; Sid = $Sid
    InstallLocation = $(if ($script:fixtureInstalled) { 'fixture-location' } else { '' })
    keys = [ordered]@{
      userProduct = @{ path = 'fixture-userProduct'; present = ($script:fixtureInstalled -and '${invalid}' -ne 'userProduct') -or '${invalid}' -eq 'leftover' }
      userData = @{ path = 'fixture-userData'; present = $script:fixtureInstalled -and '${invalid}' -ne 'userData' }
      managed = @{ path = 'fixture-managed'; present = $script:fixtureInstalled -and '${invalid}' -eq 'managed' }
      machineProduct = @{ path = 'fixture-machineProduct'; present = $script:fixtureInstalled -and '${invalid}' -eq 'machineProduct' }
    }
    uninstall = [ordered]@{ 'HKCU:' = @($entries) * ${hkcu}; "Registry::HKEY_USERS\\$Sid" = @($entries) * ${hku}; 'HKLM:' = @($entries) * ${hklm} }
  }
}
function Start-Process($FilePath, $ArgumentList, [switch]$Wait, [switch]$PassThru) {
  if ($FilePath -ne 'msiexec.exe') {
    if ($ArgumentList -cne '/S') { throw 'NSIS requires /S.' }
    if ($FilePath -eq $nsis[0].FullName) {
      Write-Host 'NSIS install.'
      $script:applicationValues[''] = Split-Path $userRuntime
      Set-Content -LiteralPath $userRuntime -Value ''
      Set-Content -LiteralPath (Join-Path (Split-Path $userRuntime) 'uninstall.exe') -Value ''
    } elseif ($FilePath -eq $nsisUninstaller) {
      Remove-Item -LiteralPath $userRuntime, $nsisUninstaller
      # Silent NSIS uninstall keeps the default install path under the shared application key.
      Write-Host "NSIS retained values: $($script:applicationValues | ConvertTo-Json -Compress)"
    } else { throw "Unexpected installer: $FilePath" }
    return [PSCustomObject]@{ ExitCode = 0 }
  }
  $script:fixtureInstalled = $ArgumentList -match '/i '
  if ($script:fixtureInstalled) {
    $script:applicationValues['InstallDir'] = (Split-Path $userRuntime) + '\\'
    Set-Content -LiteralPath $userRuntime -Value ''
    Set-Content -LiteralPath $userMsiLog -Value "Property(S): UserSID = S-1-5-21-456"
  } else {
    if ('${invalid}' -ne 'application-leftover') { $script:applicationValues.Remove('InstallDir') }
    $script:fixtureProductLeftover = '${invalid}' -eq 'product-leftover'
    Remove-Item -LiteralPath $userRuntime
  }
  return [PSCustomObject]@{ ExitCode = ${code} }
}
${sequence}`, [msi, runtime])
    expect(result.status, result.stdout + result.stderr).toBe(context === 2 && count === 1 && hkcu + hklm === 1 && code === 0 && !invalid ? 0 : 1)
    expect(result.stdout).toContain('Property(S): UserSID = S-1-5-21-456')
    expect(result.stdout).toContain(`hkcu=${hkcu} HKU\\S-1-5-21-123=${hku} hklm=${hklm}`)
    if (code !== 0) expect(result.stderr).toContain(`Silent regular MSI install failed: ${code}`)
    else if (context !== 2 || count !== 1) {
      expect(result.stderr).toContain('The MSI must register one installed, unmanaged product')
      expect(result.stdout).toContain('per-user MSI uninstalled ProductCode=')
    }
    else if (invalid) {
      const messages = {
        'application-leftover': 'The application registration remains after the per-user MSI uninstall.',
        'product-leftover': 'The MSI product registration remains after the per-user uninstall.',
      }
      expect(result.stderr).toContain(messages[invalid] ?? 'Per-user MSI registration failed')
      expect(result.stdout).toContain('per-user MSI uninstalled ProductCode=')
      expect(result.stdout).not.toContain('NSIS install.')
    } else {
      if (hkcu + hklm !== 1) expect(result.stderr).toContain('The regular MSI must have exactly one uninstall registration.')
      expect(result.stdout).toContain('The regular MSI has 1 Windows Installer registrations.')
      expect(result.stdout).toContain('The MSI registration has ProductCode={12345678-1234-ABCD-EF12-34567890ABCD} context=2 SID=S-1-5-21-123 State=5.')
      expect(result.stdout).toContain('per-user MSI uninstalled ProductCode=')
      expect(result.stdout).toContain('hkcu=0 HKU\\S-1-5-21-123=0 hklm=0')
      for (const key of ['userProduct', 'userData', 'managed', 'machineProduct']) {
        expect(result.stdout).toContain(`fixture-${key} present=0`)
      }
      if (hkcu + hklm === 1) {
        const cleanup = result.stdout.indexOf('MSI application cleanup check.')
        const productCleanup = result.stdout.indexOf('MSI product cleanup check.')
        expect(productCleanup).toBeGreaterThan(result.stdout.indexOf('per-user MSI uninstalled ProductCode='))
        expect(cleanup).toBeGreaterThan(productCleanup)
        expect(result.stdout.indexOf('NSIS install.')).toBeGreaterThan(cleanup)
        const retained = result.stdout.match(/NSIS retained values: (.+)/)
        expect(retained).not.toBeNull()
        expect(JSON.parse(retained[1])).toEqual({ '': path.dirname(runtime) })
      }
    }
  })

  it.skipIf(!hasPowerShell)('Returns only registration rows before and after uninstall.', () => {
    const msi = path.join(temp(), 'fixture.msi')
    fs.writeFileSync(msi, '')
    const result = invoke(`
${comFixture}
$installed = @(Get-MsiRegistrations $args[0] 'S-1-5-21-123')
$script:fixtureInstalled = $false
$absent = @(Get-MsiRegistrations $args[0] 'S-1-5-21-123')
[PSCustomObject]@{
  Installed = $installed; Absent = $absent; Getters = $script:getters
  Queries = $script:queries; Released = $script:released; Closed = $script:closed
} | ConvertTo-Json -Depth 5 -Compress`, [msi])
    expect(result.status, result.stdout + result.stderr).toBe(0)
    const evidence = JSON.parse(result.stdout.trim().split(/\r?\n/).at(-1))
    expect(evidence.Installed).toEqual([{
      ProductCode: '{12345678-1234-ABCD-EF12-34567890ABCD}', Context: 2, UserSid: 'S-1-5-21-123', State: 5,
    }])
    expect(evidence.Absent).toEqual([])
    expect(evidence.Getters).toEqual(['ProductCode', 'Context', 'UserSid', 'InstallProperty'])
    expect(evidence.Queries).toEqual(Array(2).fill("SELECT `Value` FROM `Property` WHERE `Property` = 'ProductCode'"))
    expect(evidence.Closed).toBe(2)
    for (const object of ['record', 'view', 'database', 'installer']) {
      expect(evidence.Released.filter((tag) => tag === object)).toHaveLength(2)
    }
  })

  it.skipIf(!hasPowerShell)('Closes the view and releases COM objects when ProductCode is absent.', () => {
    const msi = path.join(temp(), 'fixture.msi')
    fs.writeFileSync(msi, '')
    const result = invoke(`
${comFixture}
$script:fixtureRecord = $false
try { Get-MsiRegistrations $args[0] 'S-1-5-21-123' } catch { Write-Host $_.Exception.Message }
[PSCustomObject]@{ Closed = $script:closed; Released = $script:released } | ConvertTo-Json -Compress`, [msi])
    expect(result.status, result.stdout + result.stderr).toBe(0)
    expect(result.stdout).toContain('The MSI has no ProductCode.')
    expect(JSON.parse(result.stdout.trim().split(/\r?\n/).at(-1))).toEqual({
      Closed: 1, Released: ['view', 'database', 'installer'],
    })
  })

  it.skipIf(!hasPowerShell).each([
    [1, 'S-1-5-21-123', 1, 1],
    [2, 'S-1-5-21-123', 1, 0],
    [4, '', 1, 1],
    [0, 'S-1-5-21-123', 1, 1],
    [7, 'S-1-5-21-123', 1, 1],
    [2, '', 1, 1],
    [2, 'S-1-5-21-456', 1, 1],
    [2, 'S-1-5-21-123', 0, 1],
    [2, 'S-1-5-21-123', 2, 1],
  ])('Checks MSI context %s, SID %s, and registration count %s.', (context, sid, count, status) => {
    const msi = path.join(temp(), 'fixture.msi')
    fs.writeFileSync(msi, '')
    const result = invoke(`
${comFixture}
$script:fixtureContext = ${context}
$script:fixtureSid = '${sid}'
$script:fixtureCount = ${count}
$registrations = @(Get-MsiRegistrations $args[0] 'S-1-5-21-123')
Assert-MsiProductContext $registrations 'S-1-5-21-123'`, [msi])
    expect(result.status, result.stdout + result.stderr).toBe(status)
    if (status !== 0) expect(result.stderr).toContain('The MSI must register one installed, unmanaged product')
  })

  it.skipIf(!hasPowerShell).each([
    ['machine key', 'The per-user MSI wrote application registration under HKLM.'],
    ['directory', 'The per-user MSI must register its LocalAppData install path under HKCU.'],
    ['missing runtime', 'Regular MSI runtime not found'],
    ['MSI registration', 'The MSI product registration remains after the per-user uninstall.'],
    ['user key', 'The application registration remains after the per-user MSI uninstall.'],
    ['runtime', 'The runtime remains after the per-user MSI uninstall.'],
  ])('Rejects the %s failure after the scope check passes.', (failure, message) => {
    const checks = script.slice(script.indexOf('$userKey = "HKCU:'))
    const msi = path.join(temp(), 'fixture.msi')
    fs.writeFileSync(msi, '')
    const result = invoke(`
${comFixture}
$sessionSid = 'S-1-5-21-123'
$userRegistrations = @(Get-MsiRegistrations $args[0] $sessionSid)
$hkcu = @()
$hklm = @('fixture')
$regularMsi = @([PSCustomObject]@{ FullName = $args[0] })
$machineKey = 'HKLM:\\Software\\Muniment\\muniment'
$userRuntime = Join-Path ([IO.Path]::GetTempPath()) 'muniment-runtime.exe'
$script:uninstalled = $false
function Test-Path($Path) {
  if ($Path -eq $machineKey) { return '${failure}' -eq 'machine key' }
  if ($Path -eq $userRuntime) {
    if ($script:uninstalled) { return '${failure}' -eq 'runtime' }
    return '${failure}' -ne 'missing runtime'
  }
  return '${failure}' -eq 'user key'
}
function Get-ItemPropertyValue($Path, $Name) {
  if ('${failure}' -eq 'directory') { return 'wrong' }
  return Split-Path $userRuntime
}
function Invoke-Msi {
  $script:uninstalled = $true
  $script:fixtureInstalled = '${failure}' -eq 'MSI registration'
}
function Get-PerUserMsiRegistration { return @{} }
function Write-PerUserMsiRegistration {}
function Assert-PerUserMsiRegistration {}
${checks}`, [msi])
    expect(result.status, result.stdout + result.stderr).toBe(1)
    expect(result.stderr).toContain(message)
  })

  it.skipIf(!hasPowerShell).each([0, 3010, 1603])('Keeps installer exit code %s and quotes the log path.', (code) => {
    const directory = temp()
    const msi = path.join(directory, 'package [fixture].msi')
    const log = path.join(directory, 'verbose log.txt')
    fs.writeFileSync(msi, '')
    const result = invoke(`
function Start-Process($FilePath, $ArgumentList, [switch]$Wait, [switch]$PassThru) {
  Write-Host "$FilePath $ArgumentList wait=$Wait passThru=$PassThru"
  return [PSCustomObject]@{ ExitCode = ${code} }
}
Invoke-Msi "/i" $args[0] "Fixture install" $args[1]`, [msi, log])
    expect(result.status, result.stdout + result.stderr).toBe(code === 1603 ? 1 : 0)
    expect(result.stdout).toContain(`msiexec.exe /i "${msi}" /qn /norestart /L*V "${log}" wait=True passThru=True`)
    expect(result.stdout).not.toMatch(/ALLUSERS=|MSIINSTALLPERUSER=/)
    if (code === 1603) expect(result.stderr).toContain('Fixture install failed: 1603')
  })
})

describe('installed model settings controls', () => {
  const onboardingSpec = fs.readFileSync(path.join(root, 'test/e2e/specs/onboarding.spec.js'), 'utf8')

  it('opens model settings and checks the provider controls on the installed build', () => {
    expect(onboardingSpec).toContain('await openFirstRunModelSettings()')
    const firstRun = fs.readFileSync(path.join(root, 'test/e2e/support/first-run.mjs'), 'utf8')
    expect(firstRun).toContain("panel.$('button=Open model settings')")
    expect(firstRun).toContain('await settings.click()')
    expect(onboardingSpec).toContain("expect(await (await $('#provider-key')).isDisplayed()).toBe(true)")
    expect(onboardingSpec).toContain("expect(await (await $('button=Save key')).isDisplayed()).toBe(true)")
  })

  it('includes the model settings panel in the failure diagnostic', () => {
    expect(onboardingSpec).toContain("const panel = await $('#onboarding-model-panel')")
    expect(onboardingSpec).toContain('await panel.getText()')
  })
})

describe('Windows desktop executable lookup', { timeout: 30_000 }, () => {
  const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/windows.ps1'), 'utf8')
  const start = runner.lastIndexOf('\n  Install-Product') + '\n  Install-Product'.length
  const lookup = runner.slice(start, runner.indexOf('\n  Write-ToolchainState "before-e2e-build"', start))
  const native = runner.slice(runner.indexOf('function Resolve-NativeCommand'), runner.indexOf('function Get-UninstallEntries'))
  const powershell = process.platform === 'win32' ? 'powershell.exe' : 'pwsh'
  const hasPowerShell = process.platform === 'win32' || spawnSync(powershell, ['-NoProfile', '-Command', 'exit 0'], { timeout: 15_000 }).status === 0

  it('Uses the shipped desktop name for both guards.', () => {
    const cargo = fs.readFileSync(path.join(root, 'src-tauri/Cargo.toml'), 'utf8')
    const config = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8'))
    expect(cargo).toMatch(/^\[package\]\s+name = "muniment-desktop"/)
    expect(config.mainBinaryName).toBeUndefined()
    expect(lookup).toContain('$appBinary = Join-Path $installDirectory "muniment-desktop.exe"')
    expect(lookup).toContain('Test-Path -LiteralPath $appBinary -PathType Leaf')
    expect(lookup).not.toContain('$installDisplayIcon')
    expect(lookup).toContain('webdriver-release-guard.mjs absent `"$appBinary`"')
    const build = runner.indexOf('"`"$tauriCli`" build --no-bundle --features e2e-webdriver')
    const binary = runner.indexOf('"../../../src-tauri/target/release/muniment-desktop.exe"')
    expect(build).toBeGreaterThan(start)
    expect(binary).toBeGreaterThan(build)
    expect(runner.indexOf('webdriver-release-guard.mjs present `"$appBinary`"')).toBeGreaterThan(binary)
    expect(runner).not.toContain('muniment.exe')
  })

  it('Keeps WebDriver at the installed path that the runtime admits.', () => {
    const payload = fs.readFileSync(path.join(root, 'src-tauri/core/src/windows_payload.rs'), 'utf8')
    expect(payload).toContain('const DESKTOP_FILE_NAME: &str = "muniment-desktop.exe";')
    const build = runner.indexOf('  $webdriverBinary = ')
    const launch = runner.indexOf('  $env:MUNIMENT_E2E_APP_BINARY = $appBinary', build)
    expect(build).toBeGreaterThan(start)
    expect(launch).toBeGreaterThan(build)
    const staging = runner.slice(build, launch)
    expect(staging).toContain('Test-Path -LiteralPath $webdriverBinary -PathType Leaf')
    const copy = staging.indexOf('Copy-Item -LiteralPath $webdriverBinary -Destination $appBinary -Force -ErrorAction Stop')
    expect(copy).toBeGreaterThan(0)
    expect(staging.indexOf('webdriver-release-guard.mjs present `"$appBinary`"')).toBeGreaterThan(copy)
    expect(staging).not.toMatch(/\$appBinary\s*=/)
  })

  it.skipIf(!hasPowerShell).each([
    ['desktop', ['muniment-desktop.exe', 'muniment-runtime.exe', 'product.ico'], 0],
    ['wrong name', ['muniment.exe', 'muniment-runtime.exe', 'product.ico'], 1],
    ['nested desktop', ['nested/muniment-desktop.exe', 'muniment-runtime.exe'], 1],
    ['directory named executable', ['muniment-desktop.exe/child.txt'], 1],
    ['empty directory', [], 1],
    ['missing directory', [], 1],
    ['missing location', [], 1],
  ])('Checks the %s fixture and records the lookup evidence.', (fixture, files, status) => {
    const directory = temp()
    const installed = path.join(directory, 'installed app [fixture]')
    if (fixture !== 'missing directory') fs.mkdirSync(installed)
    for (const file of files) {
      const destination = path.join(installed, file)
      fs.mkdirSync(path.dirname(destination), { recursive: true })
      fs.writeFileSync(destination, 'release fixture')
    }
    const icon = path.join(directory, 'muniment-desktop.exe')
    fs.writeFileSync(icon, 'TAURI_WEBDRIVER_PORT')
    const transcript = path.join(directory, 'transcript.log')
    const script = path.join(directory, 'lookup.ps1')
    fs.writeFileSync(script, `param([string]$installDirectory, [string]$installDisplayIcon, [string]$transcript, [string]$installerLog, [string]$fixture)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
if ($fixture -eq 'missing location') { $installDirectory = $null }
${native}
Start-Transcript -LiteralPath $transcript -Force | Out-Null
try {
${lookup}
  Write-Output "Release WebDriver guard passed: $appBinary"
} catch {
  Write-Output "Lookup failed: $($_.Exception.Message)"
  exit 1
} finally {
  Stop-Transcript | Out-Null
}
`)
    const result = spawnSync(powershell, ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script,
      installed, icon, transcript, path.join(directory, 'installer.log'), fixture], {
      cwd: root, encoding: 'utf8', timeout: 20_000,
    })
    expect(result.status, result.stdout + result.stderr).toBe(status)
    const evidence = fs.readFileSync(transcript, 'utf8')
    if (status === 0) {
      expect(evidence).toContain(`The installed desktop executable is ${path.join(installed, 'muniment-desktop.exe')}`)
      expect(evidence).toContain(`Release WebDriver guard passed: ${path.join(installed, 'muniment-desktop.exe')}`)
    } else {
      expect(evidence).not.toContain('Release WebDriver guard passed:')
      const failure = evidence.indexOf('Lookup failed:')
      expect(failure).toBeGreaterThan(-1)
      if (fixture === 'missing location') {
        expect(evidence).toContain('installer metadata does not identify an install directory')
      } else {
        expect(evidence).toContain(`The installed desktop executable is missing: ${path.join(installed, 'muniment-desktop.exe')}`)
        const listing = evidence.indexOf(`InstallLocation contains these files: ${installed}`)
        expect(listing).toBeGreaterThan(-1)
        expect(listing).toBeLessThan(failure)
        for (const file of files) {
          const listed = evidence.indexOf(path.join(installed, file))
          expect(listed).toBeGreaterThan(listing)
          expect(listed).toBeLessThan(failure)
        }
      }
    }
  })
})

describe('temporary fixture paths', () => {
  it('Resolves the temporary directory alias before it returns a fixture path.', () => {
    // Native paths match child-process output when Windows TEMP uses an 8.3 alias.
    const directory = temp()
    const target = path.join(directory, 'long directory name [fixture]')
    const alias = path.join(directory, 'alias')
    fs.mkdirSync(target)
    fs.symlinkSync(target, alias, process.platform === 'win32' ? 'junction' : 'dir')
    const tmpdir = vi.spyOn(os, 'tmpdir').mockReturnValue(alias)
    try {
      const fixture = temp()
      expect(path.dirname(fixture)).toBe(target)
      expect(fixture).toBe(fs.realpathSync.native(fixture))
      expect(fs.statSync(fixture).isDirectory()).toBe(true)
    } finally {
      tmpdir.mockRestore()
    }
  })
})

describe('Windows runtime diagnostic transcript', () => {
  const helper = path.join(root, 'test/e2e/support/windows-runtime-log-tail.mjs')
  const run = (roots, env = {}) => spawnSync(process.execPath, [helper], {
    input: JSON.stringify(roots), encoding: 'utf8', env: { ...process.env, ...env },
  })
  const plant = (directory, text) => {
    const logs = path.join(directory, 'ai.muniment.desktop', 'logs')
    fs.mkdirSync(logs, { recursive: true })
    fs.writeFileSync(path.join(logs, 'runtime.log'), text)
  }

  it('Reads the known folder and redirected profiles without duplicate roots.', () => {
    const known = temp(); const redirected = temp(); const ready = temp()
    plant(known, 'event=runtime_task_registration_failed cause=RegisterTaskDefinition HRESULT(0x80070005)\n')
    plant(redirected, 'event=runtime_task_start_failed cause=RunTask HRESULT(0x80041326)\n')
    const result = run([known, redirected, ready, known, null, ''])
    expect(result.status).toBe(0)
    expect(result.stdout).toContain('RegisterTaskDefinition HRESULT(0x80070005)')
    expect(result.stdout).toContain('RunTask HRESULT(0x80041326)')
    expect(result.stdout).toContain('No runtime.log exists at this path.')
    expect(result.stdout.match(/dci: Windows runtime.log/g)).toHaveLength(3)
  })

  it('Reads diagnostics outside the install directory without collecting runtime state.', () => {
    const directory = temp()
    const install = path.join(directory, 'muniment')
    const state = path.join(directory, 'ai.muniment.desktop', 'state')
    fs.mkdirSync(install)
    fs.mkdirSync(state, { recursive: true })
    fs.writeFileSync(path.join(state, 'journal'), 'private runtime state')
    plant(directory, 'event=activation_failed message=runtime activation failed\n')
    fs.rmdirSync(install)

    const result = run([directory])
    expect(result.status).toBe(0)
    expect(result.stdout).toContain(path.join(directory, 'ai.muniment.desktop', 'logs', 'runtime.log'))
    expect(result.stdout).toContain('event=activation_failed message=runtime activation failed')
    expect(result.stdout).not.toContain('private runtime state')
    expect(fs.existsSync(install)).toBe(false)
  })

  it('Redacts the full diagnostic before the tail cutoff.', () => {
    const directory = temp()
    const secret = 'first-secret-line\nsecond-secret-line'
    plant(directory, `${secret}\n${'event=activation_failed\n'.repeat(59)}`)
    const result = run([directory], { MUNIMENT_E2E_PASSWORD: secret })
    expect(result.status).toBe(0)
    expect(result.stdout).toContain('[REDACTED]')
    expect(result.stdout).not.toContain('secret-line')
  })

  it('Names empty and oversized logs and still reads the next profile.', () => {
    const empty = temp(); const oversized = temp(); const next = temp()
    plant(empty, '')
    plant(oversized, 'x'.repeat(1024 * 1024 + 1))
    plant(next, 'event=runtime_task_start_failed cause=Task start failed\n')
    const result = run([empty, oversized, next])
    expect(result.status).toBe(0)
    expect(result.stdout).toContain('The runtime.log is empty.')
    expect(result.stdout).toContain('The runtime.log exceeds the 1 MiB diagnostic limit.')
    expect(result.stdout).toContain('cause=Task start failed')
  })

  it('Prints diagnostics before uninstall and transcript closure without copying runtime state.', () => {
    const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/windows.ps1'), 'utf8')
    const finalizer = runner.slice(runner.indexOf('function Finalize-Run'))
    expect(runner).toContain('[Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)')
    expect(runner).toContain('$roots = @($runtimeLocalAppData, $installLocalAppData, $env:LOCALAPPDATA)')
    expect(runner).toContain("Join-Path $stateRoot 'Degraded\\Local'")
    expect(runner).toContain("Join-Path $stateRoot 'Ready\\Local'")
    expect(finalizer.indexOf('Write-RuntimeDiagnostics')).toBeLessThan(finalizer.indexOf('Invoke-Cleanup "uninstall"'))
    expect(finalizer.indexOf('Write-RuntimeDiagnostics')).toBeLessThan(finalizer.indexOf('Stop-Transcript'))
    expect(runner).toContain("$inputText | ForEach-Object { Write-Host $_ }")
    expect(runner).not.toMatch(/Copy-Item[^\n]+runtime\.log/)
  })
})

describe('Windows onboarding profile isolation', () => {
  const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/windows.ps1'), 'utf8')
  const functions = runner.slice(runner.indexOf('function Get-E2eProfileDirectory'), runner.indexOf('function Remove-AuthHandler'))
  const powershell = process.platform === 'win32' ? 'powershell.exe' : 'pwsh'
  const hasPowerShell = spawnSync(powershell, ['-NoProfile', '-Command', 'exit 0'], { timeout: 15_000 }).status === 0

  it('Logs the app profile directory before the first render check.', () => {
    const onboardingSpec = fs.readFileSync(path.join(root, 'test/e2e/specs/onboarding.spec.js'), 'utf8')
    expect(onboardingSpec).toContain('window.__TAURI__.path.appConfigDir()')
    expect(onboardingSpec).toContain('`profile_directory: ${JSON.stringify(profileDirectory)}\\n`')
    expect(onboardingSpec.indexOf('`profile_directory:')).toBeLessThan(onboardingSpec.indexOf('await location.waitForDisplayed('))
    expect(onboardingSpec).toContain("setTimeout(() => reject('The app profile directory query timed out.'), 5000)")
  })

  it('Resets the known-folder onboarding files for each phase without changing the installed product.', () => {
    expect(functions).toContain('[Environment]::GetFolderPath([Environment+SpecialFolder]::UserProfile)')
    expect(functions).toContain("return Join-Path $profile '.muniment'")
    expect(functions).not.toContain('$env:APPDATA')
    expect(functions).not.toContain('$env:USERPROFILE')
    expect(runner).toContain("'test/e2e/specs/local-mode-chat.spec.js' -ProfileState first-run")
    expect(runner).toContain("'test/e2e/specs/real-sign-in.spec.js' -ProfileState signed-out")
    expect(runner).toContain('"Windows onboarding tests failed" -ProfileState first-run')
    expect(functions.indexOf('if ($script:cleanupLastStatus -ne 0)')).toBeLessThan(functions.indexOf('$profileDirectory = Get-E2eProfileDirectory'))
    expect(functions).not.toContain('Remove-Item -LiteralPath $profileDirectory')
  })

  it.skipIf(!hasPowerShell).each([
    ['missing', 'first-run', false, true],
    ['configured', 'first-run', false, true],
    ['configured', 'signed-out', false, true],
    ['configured', 'preserve', false, true],
    ['directory', 'first-run', false, false],
    ['configured', 'first-run', true, false],
    ['removal-failure', 'first-run', false, false],
    ['configured', 'invalid', false, false],
  ])('Handles %s state with %s and stop failure %s.', (state, profileState, stopFails, starts) => {
    const directory = temp()
    const profile = path.join(directory, 'known folder [fixture]', '.muniment')
    const redirected = path.join(directory, 'redirected', '.muniment')
    fs.mkdirSync(redirected, { recursive: true })
    fs.writeFileSync(path.join(redirected, 'home.json'), 'redirected Home')
    if (state !== 'missing') {
      fs.mkdirSync(profile, { recursive: true })
      fs.writeFileSync(path.join(profile, 'home.json'), 'configured Home')
      fs.writeFileSync(path.join(profile, 'session.json'), 'session for revocation')
      fs.writeFileSync(path.join(profile, 'runtime.db'), 'runtime data')
      if (state === 'directory') fs.mkdirSync(path.join(profile, 'local-mode'))
      else fs.writeFileSync(path.join(profile, 'local-mode'), '1')
    }
    const script = path.join(directory, 'profile.ps1')
    fs.writeFileSync(script, `param($Directory, $Profile, $ProfileState, $StopFails, $State)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$raw = $Directory
$cleanupLog = Join-Path $Directory 'cleanup.log'
$cleanupStatusLedger = Join-Path $Directory 'cleanup-status.log'
$cleanupStatus = 0
${functions}
function Get-E2eProfileDirectory { return $Profile }
function Stop-HarnessProcesses {
  if ($StopFails -eq 'true') { throw 'The fixture process did not stop.' }
}
if ($State -eq 'removal-failure') {
  function Remove-Item { throw 'The fixture file is locked.' }
}
function Invoke-NativeCommand {
  Set-Content -LiteralPath (Join-Path $Directory 'started') -Value '1'
}
try { Invoke-E2e (Join-Path $raw 'spec.log') 'The fixture spec failed.' -ProfileState $ProfileState }
catch { Write-Output $_.Exception.Message; exit 1 }
`)
    const result = spawnSync(powershell, ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script,
      directory, profile, profileState, String(stopFails), state], {
      encoding: 'utf8', timeout: 20_000,
      env: { ...process.env, APPDATA: path.dirname(redirected), MUNIMENT_E2E_FINALIZER_TEST_MODE: '0', MUNIMENT_E2E_FINALIZER_TEST_FAIL: '' },
    })
    expect(result.status, result.stdout + result.stderr).toBe(starts ? 0 : 1)
    expect(fs.existsSync(path.join(directory, 'started'))).toBe(starts)
    expect(fs.readFileSync(path.join(redirected, 'home.json'), 'utf8')).toBe('redirected Home')
    if (state === 'missing') {
      expect(fs.existsSync(profile)).toBe(false)
    } else {
      expect(fs.existsSync(path.join(profile, 'home.json'))).toBe(!starts || profileState !== 'first-run')
      expect(fs.existsSync(path.join(profile, 'local-mode'))).toBe(!starts || profileState === 'preserve')
      expect(fs.readFileSync(path.join(profile, 'session.json'), 'utf8')).toBe('session for revocation')
      expect(fs.readFileSync(path.join(profile, 'runtime.db'), 'utf8')).toBe('runtime data')
    }
    if (starts && profileState !== 'preserve') {
      const log = fs.readFileSync(path.join(directory, 'cleanup.log'), 'utf8')
      expect(log).toContain(`profile_directory=${profile} profile_state=${profileState}`)
    }
  }, 30_000)
})

describe('Windows sign-in clock', () => {
  const helper = path.join(root, 'test/e2e/support/windows-clock.ps1')
  const powershell = process.platform === 'win32' ? 'powershell.exe' : 'pwsh'
  const hasPowerShell = spawnSync(powershell, ['-NoProfile', '-Command', 'exit 0'], { timeout: 15_000 }).status === 0

  it('checks the clock after process cleanup and before the sign-in spec', () => {
    const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/windows.ps1'), 'utf8')
    const invoke = runner.slice(runner.indexOf('function Invoke-E2e('), runner.indexOf('function Invoke-Cleanup('))
    expect(invoke).toContain("if ($Spec -eq 'test/e2e/specs/real-sign-in.spec.js')")
    expect(invoke.indexOf('Stop-HarnessProcesses')).toBeLessThan(invoke.indexOf('Sync-SignInClock'))
    expect(invoke.indexOf('Sync-SignInClock')).toBeLessThan(invoke.indexOf('Invoke-NativeCommand'))
    const clock = fs.readFileSync(helper, 'utf8')
    expect(clock).toContain("$source = 'https://api.muniment.ai/'")
    expect(clock).toContain('$request.AllowAutoRedirect = $false')
    expect(clock).toContain('$request.Timeout = 10000')
    expect(clock).toContain('TryParseExact')
    expect(clock).not.toMatch(/ServerCertificateValidationCallback|SkipCertificateCheck/)
  })

  it.skipIf(!hasPowerShell).each([
    ['ahead', -10800, 0, false, true, 0],
    ['behind', 10800, 0, false, true, 0],
    ['boundary', 9, 9, false, false, 0],
    ['outside', 9.1, 0, false, true, 0],
    ['uncorrected', 10800, 10800, false, true, 1],
    ['denied', 10800, 0, true, true, 1],
    ['unavailable', 0, 0, false, false, 1],
  ])('verifies the %s clock without changing the host clock', (name, before, after, denied, setsClock, status) => {
    const directory = temp()
    const script = path.join(directory, 'clock-test.ps1')
    fs.writeFileSync(script, `param([string]$Helper, [string]$Directory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. $Helper
$script:calls = 0
function Get-TrustedClockSample {
  if ('${name}' -eq 'unavailable') { throw 'The trusted clock source did not answer over HTTPS.' }
  $offset = if ($script:calls -eq 0) { ${before} } else { ${after} }
  $script:calls += 1
  return [pscustomobject]@{ Source = 'https://api.muniment.ai/'; LocalUtc = [DateTime]::UtcNow; TrustedUtc = [DateTime]::UtcNow.AddSeconds($offset); OffsetSeconds = $offset; UncertaintySeconds = 1 }
}
function Set-Date {
  param($Date, $ErrorAction)
  Set-Content (Join-Path $Directory 'set-date') 'called'
  if ($${denied}) { throw 'Access denied.' }
}
try { Sync-SignInClock (Join-Path $Directory 'clock.log'); exit 0 }
catch { Write-Output $_.Exception.Message; exit 1 }
`)
    const result = spawnSync(powershell, ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script, helper, directory], { encoding: 'utf8', timeout: 20_000 })
    expect(result.status, result.stdout + result.stderr).toBe(status)
    expect(fs.existsSync(path.join(directory, 'set-date'))).toBe(setsClock)
    const log = fs.readFileSync(path.join(directory, 'clock.log'), 'utf8')
    if (status === 0) {
      expect(log).toContain('phase=before')
      expect(log).toContain('phase=verified')
      expect(log).toContain('trusted_utc=')
    } else {
      expect(log).toContain('clock failure=')
    }
  }, 30_000)
})

describe.skipIf(process.platform === 'win32')('macOS WDIO spec homes', () => {
  it('Redirects each spec home while the login home keeps the runtime.', () => {
    const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/macos-wdio.sh'), 'utf8')
    const sequence = runner.slice(runner.indexOf('# Each spec starts'))
    const specs = ['local-mode-chat', 'real-sign-in', 'onboarding', 'cleanup']
    const directory = temp()
    const home = path.join(directory, 'login home')
    const stateRoot = path.join(directory, 'state')
    const result = spawnSync('bash', ['-c', `
set -uo pipefail
state_root="$FIXTURE_STATE_ROOT"
raw="$state_root/raw"
run_e2e() {
  printf '%s\\n' "$1" "$HOME" "$MUNIMENT_E2E_HOME_PATH"
}
${sequence}
`], {
      encoding: 'utf8', timeout: 10_000,
      env: { ...process.env, HOME: home, FIXTURE_STATE_ROOT: stateRoot },
    })
    expect(result.status, result.stderr).toBe(0)
    expect(result.stdout.trim().split('\n')).toEqual(specs.flatMap((spec, index) => {
      const phase = index < 2 ? 'degraded' : 'ready'
      return [spec, path.join(stateRoot, phase), path.join(stateRoot, `${phase}-home`)]
    }))
    // The login home's state root links to each spec home, so no data-home variable is needed.
    expect(sequence).not.toContain('XDG_DATA_HOME')
  })
})

describe('Windows folder dialog diagnostics', () => {
  it.skipIf(process.platform !== 'win32')('The timeout reports hidden and offscreen app and WebView2 windows.', () => {
    const directory = temp()
    const result = spawnSync('powershell.exe', [
      '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File',
      path.join(root, 'test/e2e/fixtures/folder-dialog-windows.ps1'), directory,
      path.join(root, 'test/e2e/support/folder-dialog-windows.ps1'),
    ], { encoding: 'utf8', timeout: 60_000 })
    const output = result.stdout + result.stderr
    expect(result.status, output).toBe(0)
    expect(output).toContain('driver exit: 1')
    expect(output).toContain('The shell folder dialog timed out during the window search.')
    expect(output).not.toMatch(/window report error:|process tree error:/)
    const windows = output.split(/\r?\n/).filter((line) => line.startsWith('window: '))
      .map((line) => JSON.parse(line.slice('window: '.length)))
    for (const prefix of ['app', 'webview']) {
      const processId = Number(fs.readFileSync(path.join(directory, `${prefix}.ready`), 'utf8'))
      for (const state of ['visible', 'hidden', 'offscreen']) {
        const window = windows.find((entry) => entry.Title === `${prefix} ${state}` && entry.ProcessId === processId)
        expect(window, output).toBeDefined()
        expect(window.Class).toMatch(/^WindowsForms/)
        expect(window.Visible).toBe(state !== 'hidden')
        expect(window.RectangleAvailable).toBe(true)
        expect(window.Rectangle.Right).toBeGreaterThan(window.Rectangle.Left)
        expect(window.Rectangle.Bottom).toBeGreaterThan(window.Rectangle.Top)
        expect(typeof window.Offscreen).toBe('boolean')
        if (state === 'offscreen') {
          expect(window.Offscreen).toBe(true)
          expect(window.Rectangle.Left).toBe(-32000)
        }
      }
    }
  }, 70_000)
})
