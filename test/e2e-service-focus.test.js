import { afterEach, expect, it, vi } from 'vitest'
import TauriService from '@wdio/tauri-service'
import os from 'node:os'
import path from 'node:path'

// Use the installed service, not a mock of its window selection policy.
it('The harness pins the service target without repeated focus probes.', async () => {
  vi.stubEnv('MUNIMENT_E2E_APP_BINARY', path.join(os.tmpdir(), 'muniment-test-binary'))
  vi.stubEnv('MUNIMENT_E2E_RAW_DIR', os.tmpdir())
  const { config } = await import('./e2e/wdio.conf.js')
  let handle = 'launcher'
  const driver = {
    sessionId: 'onboarding-focus-regression',
    getWindowHandle: async () => handle,
    getWindowHandles: async () => ['launcher', 'main'],
    switchToWindow: vi.fn(async (next) => { handle = next }),
    execute: async () => handle,
    executeAsync: async () => undefined,
    overwriteCommand: vi.fn(),
    waitUntil: async (predicate) => { expect(await predicate()).toBe(true) },
  }
  const service = new TauriService({ driverProvider: 'embedded' }, {})
  await service.before({}, [], driver)
  vi.stubGlobal('browser', driver)
  const probe = vi.spyOn(driver.tauri, 'execute').mockRejectedValue(new Error('The companion plugin is absent.'))

  await service.beforeCommand('$', [])
  expect(probe).toHaveBeenCalledTimes(1)
  await config.beforeTest()
  expect(handle).toBe('main')
  probe.mockClear()
  for (const command of ['$', 'findElement', 'elementClick', '$$', 'findElements', 'getTitle']) {
    await service.beforeCommand(command, [])
  }
  expect(probe).not.toHaveBeenCalled()
  expect(handle).toBe('main')
})

it('The harness keeps the failure source when a later test changes the screen.', async () => {
  vi.resetModules()
  vi.stubEnv('MUNIMENT_E2E_APP_BINARY', path.join(os.tmpdir(), 'muniment-test-binary'))
  vi.stubEnv('MUNIMENT_E2E_RAW_DIR', os.tmpdir())
  const { config, captureFailureArtifacts } = await import('./e2e/wdio.conf.js')
  const getPageSource = vi.fn()
  vi.stubGlobal('browser', { getPageSource, getWindowHandles: vi.fn() })
  const writeFile = vi.fn()
  await captureFailureArtifacts({ passed: false }, {
    selectMainWindow: async () => {},
    getPageSource: async () => '<main>The runtime is not connected yet.</main>',
    saveScreenshot: async () => {},
    writeFile,
    log: vi.fn(),
  })
  await config.after(1)
  expect(writeFile).toHaveBeenCalledTimes(1)
  expect(browser.getWindowHandles).not.toHaveBeenCalled()
  expect(getPageSource).not.toHaveBeenCalled()
})

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.unstubAllEnvs()
})
