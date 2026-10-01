import { test } from 'node:test'
import assert from 'node:assert/strict'
import { macosCaptureInfo, screenshot, ScreenshotError, screenshotReasons } from './e2e/runner/subscriptions.mjs'

const pid = 123
const window = { id: 456, pid, layer: 0, onscreen: true, x: -100, y: 0, width: 800, height: 600, sharing_state: 1 }
const snapshot = (access = true, windows = [window]) => ({ screen_capture_access: access, windows })
const imageFailure = 'screencapture: status=1 signal=none error=none\nstderr:\ncould not create image from window'

function capture({ before = snapshot(), after = before, failure, compileFailure, platform = 'macos-x64' } = {}) {
  const calls = []
  let inspections = 0
  const execute = (command, args, env, timeout) => {
    calls.push({ command, args, env, timeout })
    if (command === 'clang') {
      if (compileFailure) throw new Error('The compiler failed.')
      return ''
    }
    if (command === 'screencapture') {
      if (failure) throw new Error(failure)
      return ''
    }
    assert.deepEqual(args, [String(pid), '--capture-info'])
    const info = inspections++ === 0 ? before : after
    if (info instanceof Error) throw info
    return JSON.stringify(info)
  }
  let error
  try { screenshot(platform, pid, '/tmp/capture.png', { TMPDIR: '/tmp/probe' }, execute) }
  catch (caught) { error = caught }
  return { error, calls, inspections }
}

function assertReason(result, code) {
  assert.ok(result.error instanceof ScreenshotError)
  assert.equal(result.error.reason, screenshotReasons[code])
  assert.ok(result.error.message.includes(`capture=${code}`))
}

for (const platform of ['macos-x64', 'macos-arm64']) {
  test(`${platform} retains the native window screenshot`, () => {
    const result = capture({ platform })
    assert.equal(result.error, undefined)
    const call = result.calls.find(call => call.command === 'screencapture')
    assert.deepEqual(call.args, ['-x', '-l456', '/tmp/capture.png'])
    assert.equal(call.timeout, 10_000)
    assert.equal(result.inspections, 1)
  })
}

test('a denied helper preflight does not veto a successful capture', () => {
  assert.equal(capture({ before: snapshot(false) }).error, undefined)
})

test('a failed capture reports the denied preflight and window metadata', () => {
  const result = capture({ before: snapshot(false), failure: imageFailure })
  assertReason(result, 'permission-denied')
  assert.equal(result.inspections, 2)
  assert.ok(result.error.message.includes(`before=${JSON.stringify(snapshot(false))}`))
  assert.ok(result.error.message.includes('"sharing_state":1'))
  assert.ok(result.error.message.includes(imageFailure))
})

test('a granted preflight separates an uncapturable image from permission denial', () => {
  const result = capture({ failure: imageFailure })
  assertReason(result, 'window-uncapturable')
  assert.ok(result.error.message.includes(`after=${JSON.stringify(snapshot())}`))
})

test('capture tool failures do not claim an uncapturable image', () => {
  for (const failure of ['spawnSync screencapture ENOENT', 'screencapture: signal=SIGTERM']) {
    assertReason(capture({ failure }), 'capture-failed')
  }
})

for (const windows of [[], [window, { ...window, id: 789 }]]) {
  test(`the capture rejects ${windows.length} matching windows`, () => {
    const result = capture({ before: snapshot(true, windows) })
    assertReason(result, 'window-unavailable')
    assert.equal(result.calls.some(call => call.command === 'screencapture'), false)
  })
}

test('a failed capture checks for a vanished or replaced window', () => {
  for (const windows of [[], [{ ...window, id: 789 }], [window, { ...window, id: 789 }]]) {
    assertReason(capture({ failure: imageFailure, after: snapshot(true, windows) }), 'window-unavailable')
  }
})

test('a failed capture uses the current permission check', () => {
  assertReason(capture({ failure: imageFailure, after: snapshot(false) }), 'permission-denied')
  assertReason(capture({ before: snapshot(false), after: snapshot(), failure: imageFailure }), 'window-uncapturable')
})

test('missing diagnostics retain the capture error without guessing its cause', () => {
  for (const options of [{ compileFailure: true }, { before: null }, { before: new Error('The window list timed out.') }]) {
    assertReason(capture(options), 'diagnostics-unavailable')
  }
  const result = capture({ failure: imageFailure, after: new Error('The window list timed out.') })
  assertReason(result, 'diagnostics-unavailable')
  assert.ok(result.error.message.includes(imageFailure))
  assert.ok(result.error.message.includes('The window list timed out.'))
})

test('capture metadata rejects malformed or contradictory fields', () => {
  for (const patch of [
    { id: 0 }, { id: -1 }, { id: 0x100000000 }, { id: 1.5 }, { id: '456' },
    { pid: 999 }, { layer: 1 }, { onscreen: false }, { width: 0 }, { height: -1 },
    { x: null }, { y: '0' }, { sharing_state: 3 }, { sharing_state: null },
  ]) {
    assert.throws(() => macosCaptureInfo(JSON.stringify(snapshot(true, [{ ...window, ...patch }])), pid))
  }
  for (const info of [null, {}, snapshot('true'), snapshot(true, null), snapshot(true, [null]), snapshot(true, [window, window])]) {
    assert.throws(() => macosCaptureInfo(JSON.stringify(info), pid))
  }
  assert.throws(() => macosCaptureInfo('{', pid))
  for (const invalidPid of [0, -1, 1.5, 2147483648, '123']) {
    assert.throws(() => macosCaptureInfo(JSON.stringify(snapshot(true, [{ ...window, pid: invalidPid }])), invalidPid))
  }
})

test('capture metadata keeps numeric boundaries but drops names and titles', () => {
  const info = snapshot(true, [{ ...window, id: 0xffffffff, width: 1, height: 1, sharing_state: -1 }])
  assert.deepEqual(macosCaptureInfo(JSON.stringify(info), pid), info)
  assert.deepEqual(macosCaptureInfo(JSON.stringify({ ...snapshot(), name: 'private',
    windows: [{ ...window, title: 'private' }] }), pid), snapshot())
})
