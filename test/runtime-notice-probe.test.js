// @vitest-environment jsdom
import { readFileSync } from 'node:fs'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

const probe = readFileSync('src-tauri/src/macos_runtime_notice_probe.rs', 'utf8').match(/r#"([\s\S]*?)"#/)?.[1]
const runner = readFileSync('test/e2e/runner/macos.sh', 'utf8')

beforeEach(() => {
  vi.useFakeTimers()
  window.__TAURI__ = { core: { invoke: vi.fn().mockResolvedValue(undefined) } }
})

afterEach(() => {
  vi.clearAllTimers()
  vi.useRealTimers()
  vi.restoreAllMocks()
  document.body.innerHTML = ''
  delete window.__TAURI__
})

it.each(['The runtime connection closed.', 'The runtime exited.'])('reads the installed loss notice: %s', async (text) => {
  document.body.innerHTML = `<section data-testid="runtime-notice"><p>${text}</p><button>Start runtime</button></section>`
  const notice = document.querySelector('section')
  vi.spyOn(notice, 'getClientRects').mockReturnValue([{}])
  window.eval(probe)
  await vi.advanceTimersByTimeAsync(250)
  expect(window.__TAURI__.core.invoke).toHaveBeenCalledExactlyOnceWith('runtime_notice_observed', {
    text, control: 'Start runtime', controls: 1,
  })
  await vi.advanceTimersByTimeAsync(1000)
  expect(window.__TAURI__.core.invoke).toHaveBeenCalledTimes(5)
})

it('does not mistake an absent or hidden notice for a rendered notice', async () => {
  window.eval(probe)
  await vi.advanceTimersByTimeAsync(250)
  document.body.innerHTML = '<section data-testid="runtime-notice"><p>The runtime connection closed.</p><button>Start runtime</button></section>'
  await vi.advanceTimersByTimeAsync(250)
  expect(window.__TAURI__.core.invoke).not.toHaveBeenCalled()
})

it('does not count the initial start notice as the service absence probe', async () => {
  document.body.innerHTML = '<section data-testid="runtime-notice"><p>The runtime received a start request.</p><button>Start runtime</button></section>'
  vi.spyOn(document.querySelector('section'), 'getClientRects').mockReturnValue([{}])
  window.eval(probe)
  await vi.advanceTimersByTimeAsync(250)
  expect(window.__TAURI__.core.invoke).not.toHaveBeenCalled()
})

it('checks the installed webview after it removes the service from launchd', () => {
  expect(runner).toContain('"$installed_bundle/Contents/MacOS/$process_name" --probe-runtime-notice')
  expect(runner.indexOf('launchctl bootout "$runtime_target"')).toBeLessThan(runner.indexOf('notice_deadline='))
  expect(runner).toContain("grep -Ex 'runtime_notice=The runtime (connection closed|exited)[.] control=Start runtime controls=1'")
  expect(runner).toContain('tail -n "+$((notice_offset + 1))"')
  expect(runner).toContain('if (( notice_read == 0 )); then')
  expect(runner).toContain(`printf '%s\\n' "$notice_line"`)
})
