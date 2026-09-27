import { test, mock } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { prepareLinuxSandbox, linuxSandbox } from './e2e/runner/subscription-linux-sandbox.mjs'

function fixture(work, { helper = 'regular', existing = false, fail, mode = '0:0:4755', mount = 'rw,relatime' } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-sandbox-'))
  const packageFile = path.join(root, 'signed.AppImage')
  fs.writeFileSync(packageFile, 'signed bytes')
  const calls = []
  const lstat = fs.lstatSync
  const stub = mock.method(fs, 'lstatSync', (file, ...args) => {
    if (file === linuxSandbox) {
      if (existing) return { isFile: () => true }
      throw Object.assign(new Error('missing'), { code: 'ENOENT' })
    }
    if (helper === 'symlink' && String(file).endsWith('/chrome-sandbox')) return { isFile: () => false, size: 10 }
    return lstat(file, ...args)
  })
  const spawn = (command, args, options) => {
    calls.push({ command, args, options })
    assert.ok(fs.existsSync(options.cwd))
    assert.deepEqual(Object.keys(options.env).sort(), ['LANG', 'PATH'])
    assert.equal(options.timeout, 60_000)
    const operation = command === packageFile ? 'extract' : command === 'sudo' ? args[1] : command
    if (operation === fail) return { status: 1, stderr: 'fixture failure' }
    if (command === packageFile) {
      assert.deepEqual(args, ['--appimage-extract', 'usr/lib/muniment/cef/chrome-sandbox'])
      const source = path.join(options.cwd, 'squashfs-root/usr/lib/muniment/cef/chrome-sandbox')
      fs.mkdirSync(path.dirname(source), { recursive: true })
      if (helper === 'directory') fs.mkdirSync(source)
      else if (helper !== 'missing') fs.writeFileSync(source, helper === 'empty' ? '' : 'signed helper')
    }
    return { status: 0, stdout: command === 'stat' ? mode : command === 'findmnt' ? mount : '' }
  }
  try { work({ root, packageFile, calls, spawn }) }
  finally {
    stub.mock.restore()
    fs.rmSync(root, { recursive: true, force: true })
  }
}

test('the guest installs the signed sandbox helper outside FUSE without changing the AppImage', () => fixture(({ root, packageFile, calls, spawn }) => {
  const cleanup = prepareLinuxSandbox(packageFile, root, spawn)
  const source = path.join(root, 'sandbox/squashfs-root/usr/lib/muniment/cef/chrome-sandbox')
  assert.deepEqual(calls.map(call => call.command), [packageFile, 'sudo', 'stat', 'findmnt', 'cmp'])
  assert.deepEqual(calls[1].args, ['-n', 'install', '-D', '-o', 'root', '-g', 'root', '-m', '4755', '--', source, linuxSandbox])
  assert.deepEqual(calls[2].args, ['--format=%u:%g:%a', linuxSandbox])
  assert.deepEqual(calls[3].args, ['--noheadings', '--output', 'OPTIONS', '--target', linuxSandbox])
  assert.deepEqual(calls[4].args, ['--silent', '--', source, linuxSandbox])
  assert.equal(fs.readFileSync(packageFile, 'utf8'), 'signed bytes')
  assert.equal(fs.existsSync(path.join(root, 'sandbox')), false)
  cleanup()
  assert.deepEqual(calls.at(-1).args, ['-n', 'rm', '-f', '--', linuxSandbox])
}))

for (const helper of ['missing', 'empty', 'directory', 'symlink']) {
  test(`the guest rejects the ${helper} sandbox helper before privilege changes`, () => fixture(({ root, packageFile, calls, spawn }) => {
    assert.throws(() => prepareLinuxSandbox(packageFile, root, spawn))
    assert.equal(calls.length, 1)
  }, { helper }))
}

test('the guest preserves an existing sandbox helper', () => fixture(({ root, packageFile, calls, spawn }) => {
  assert.throws(() => prepareLinuxSandbox(packageFile, root, spawn), /already contains/)
  assert.equal(calls.length, 1)
}, { existing: true }))

for (const options of [
  { fail: 'install' }, { fail: 'stat' }, { fail: 'findmnt' }, { fail: 'cmp' },
  { mode: '1000:0:4755' }, { mode: '0:1000:4755' }, { mode: '0:0:755' }, { mode: '0:0:4777' },
  { mount: 'rw,nosuid,nodev' }, { mount: 'rw,noexec' }, { mount: '' },
]) {
  test(`sandbox setup fails closed and cleans up for ${JSON.stringify(options)}`, () => fixture(({ root, packageFile, calls, spawn }) => {
    assert.throws(() => prepareLinuxSandbox(packageFile, root, spawn))
    assert.deepEqual(calls.at(-1).args, ['-n', 'rm', '-f', '--', linuxSandbox])
    assert.equal(fs.existsSync(path.join(root, 'sandbox')), false)
  }, options))
}

test('extraction failures stop before privilege changes', () => fixture(({ root, packageFile, calls, spawn }) => {
  assert.throws(() => prepareLinuxSandbox(packageFile, root, spawn), /fixture failure/)
  assert.equal(calls.length, 1)
}, { fail: 'extract' }))

test('temporary cleanup failures also remove the privileged helper', () => fixture(({ root, packageFile, calls, spawn }) => {
  const remove = fs.rmSync
  const stub = mock.method(fs, 'rmSync', (file, options) => {
    if (file === path.join(root, 'sandbox')) throw new Error('Temporary cleanup failed.')
    return remove(file, options)
  })
  try {
    assert.throws(() => prepareLinuxSandbox(packageFile, root, spawn), /Temporary cleanup failed/)
    assert.deepEqual(calls.at(-1).args, ['-n', 'rm', '-f', '--', linuxSandbox])
  } finally { stub.mock.restore() }
}))

test('sandbox cleanup failures reach the caller', () => fixture(({ root, packageFile, spawn }) => {
  const cleanup = prepareLinuxSandbox(packageFile, root, spawn)
  assert.throws(cleanup, /fixture failure/)
}, { fail: 'rm' }))

test('the guest prepares the sandbox only after signature and signed name checks', () => {
  const guest = fs.readFileSync('test/e2e/runner/subscription-guest.mjs', 'utf8')
  const signature = guest.indexOf('const comment = verifyUpdaterSignature(')
  const name = guest.indexOf("if (!comment.split('\\t').includes(`file:${stableName}`))")
  const prepare = guest.indexOf('cleanupSandbox = prepareLinuxSandbox(')
  const probe = guest.indexOf('return await run(')
  assert.ok(signature > 0 && signature < name && name < prepare && prepare < probe)
  assert.match(guest, /finally \{\s+try \{ cleanupSandbox\?\.\(\) \}/)
})
