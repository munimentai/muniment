import { test as nodeTest } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'

const test = (name, fn) => nodeTest(name, { skip: process.platform !== 'linux' }, fn)
const script = fs.readFileSync('src-tauri/packaging/appimage/setup-sandbox.sh', 'utf8')

// Exercise the shipped shell logic with real files and modes, but simulated root ownership.
function fixture(work, { uid = 0, owner = 0, mount = 'rw,relatime', installFails = false } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'appimage-sandbox-'))
  const image = path.join(root, 'squashfs-root')
  const host = path.join(root, 'host')
  const directory = path.join(host, 'usr/lib/muniment/cef')
  const destination = path.join(directory, 'chrome-sandbox')
  const helper = path.join(image, 'usr/lib/muniment/cef/chrome-sandbox')
  fs.mkdirSync(path.dirname(helper), { recursive: true })
  fs.mkdirSync(host)
  fs.writeFileSync(helper, 'signed sandbox helper')
  const setup = path.join(image, 'setup-sandbox.sh')
  const doubles = `
id() { printf '%s\\n' '${uid}'; }
stat() {
  for file do :; done
  case "$2" in
    %u) printf '%s\\n' '${owner}' ;;
    %u:%g:%a) printf '0:0:%s\\n' "$(/usr/bin/stat -c %a -- "$file")" ;;
    *) /usr/bin/stat "$@" ;;
  esac
}
findmnt() { printf '%s\\n' '${mount}'; }
install() {
  ${installFails ? 'echo "Fixture install failure." >&2; return 1' : 'shift 4; /usr/bin/install "$@"'}
}
`
  const copy = script
    .replace('helper="$source_dir/usr/lib/muniment/cef/chrome-sandbox"', 'helper="SOURCE_HELPER"')
    .replaceAll('/usr/lib/muniment/cef', directory)
    .replace('SOURCE_HELPER', helper)
    .replace('for directory in /usr /usr/lib /usr/lib/muniment ', `for directory in ${host}/usr ${host}/usr/lib ${host}/usr/lib/muniment `)
    .replace('export PATH', `export PATH\n${doubles}`)
  fs.writeFileSync(setup, copy)
  const run = () => spawnSync('/bin/sh', [setup], { encoding: 'utf8', timeout: 10_000 })
  try { work({ root, helper, directory, destination, run }) }
  finally { fs.rmSync(root, { recursive: true, force: true }) }
}

test('AppImage setup installs its own helper without a DEB and supports repeated setup', () => fixture(({ helper, directory, destination, run }) => {
  assert.equal(fs.existsSync(destination), false)
  for (let attempt = 0; attempt < 2; attempt++) {
    const result = run()
    assert.equal(result.status, 0, result.stderr)
    assert.equal(fs.readFileSync(destination, 'utf8'), fs.readFileSync(helper, 'utf8'))
    assert.equal(fs.statSync(destination).mode & 0o7777, 0o4755)
    assert.deepEqual(fs.readdirSync(directory), ['chrome-sandbox'])
  }
}))

for (const options of [{ uid: 1000 }, { owner: 1000 }, { mount: '' }, { mount: 'rw,nosuid,nodev' }, { mount: 'rw,noexec' }, { installFails: true }]) {
  test(`AppImage setup fails closed for ${JSON.stringify(options)}`, () => fixture(({ directory, destination, run }) => {
    const result = run()
    assert.notEqual(result.status, 0)
    const reason = options.uid ? /with sudo/ : options.owner ? /Root must own/ : options.installFails ? /Fixture install failure/ : /setuid-enabled executable filesystem/
    assert.match(result.stderr, reason)
    assert.equal(fs.existsSync(destination), false)
    if (fs.existsSync(directory)) assert.deepEqual(fs.readdirSync(directory), [])
  }, options))
}

for (const kind of ['missing', 'empty', 'directory', 'symlink']) {
  test(`AppImage setup rejects a ${kind} source helper`, () => fixture(({ helper, destination, run }) => {
    fs.unlinkSync(helper)
    if (kind === 'empty') fs.writeFileSync(helper, '')
    if (kind === 'directory') fs.mkdirSync(helper)
    if (kind === 'symlink') fs.symlinkSync('/bin/true', helper)
    const result = run()
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /lacks a regular Chromium sandbox helper/)
    assert.equal(fs.existsSync(destination), false)
  }))
}

for (const kind of ['different', 'symlink', 'writable', 'locked', 'unsafe-directory', 'linked-directory']) {
  test(`AppImage setup preserves the host for ${kind}`, () => fixture(({ root, directory, destination, run }) => {
    fs.mkdirSync(directory, { recursive: true })
    if (kind === 'different' || kind === 'writable') {
      fs.writeFileSync(destination, kind === 'different' ? 'existing DEB helper' : 'signed sandbox helper', { mode: kind === 'different' ? 0o4755 : 0o4777 })
      fs.chmodSync(destination, kind === 'different' ? 0o4755 : 0o4777)
    }
    if (kind === 'symlink') fs.symlinkSync('/bin/true', destination)
    if (kind === 'locked') fs.mkdirSync(path.join(directory, '.sandbox-setup.lock'))
    if (kind === 'unsafe-directory') fs.chmodSync(directory, 0o777)
    if (kind === 'linked-directory') {
      const target = path.join(root, 'elsewhere')
      fs.mkdirSync(target)
      fs.rmdirSync(directory)
      fs.symlinkSync(target, directory)
    }
    const before = fs.readdirSync(directory)
    const result = run()
    assert.notEqual(result.status, 0)
    const reason = kind === 'locked' ? /holds the lock/ : kind === 'unsafe-directory' ? /Only root may write/ : kind === 'linked-directory' ? /must not be a symbolic link/ : /different sandbox helper exists/
    assert.match(result.stderr, reason)
    assert.deepEqual(fs.readdirSync(directory), before)
    if (kind === 'different') assert.equal(fs.readFileSync(destination, 'utf8'), 'existing DEB helper')
    if (kind === 'symlink') assert.equal(fs.readlinkSync(destination), '/bin/true')
  }))
}
