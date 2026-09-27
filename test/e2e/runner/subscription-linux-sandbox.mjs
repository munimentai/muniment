import fs from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { nativeFailure } from '../support/subscription-diagnostics.mjs'

export const linuxSandbox = '/usr/lib/muniment/cef/chrome-sandbox'
export const sandboxRequirement = 'The Linux guest requires a root-owned Chromium sandbox helper on a setuid-enabled filesystem.'

// Call only after the updater signature and package name checks pass.
export function prepareLinuxSandbox(packageFile, root, spawnProcess = spawnSync) {
  // The installed probe doubles as the restricted-namespace startup regression.
  const namespace = spawnProcess('unshare', ['--user', '--map-root-user', 'true'], {
    cwd: root, env: { PATH: process.env.PATH, LANG: 'C' }, timeout: 60_000, encoding: 'utf8',
  })
  if (namespace.error || namespace.status !== 1 || !String(namespace.stderr).includes('Operation not permitted')) {
    throw new Error('The Linux regression guest must deny unprivileged user namespaces.')
  }
  const expanded = path.join(root, 'sandbox')
  fs.mkdirSync(expanded)
  const execute = (command, args, cwd = root) => {
    const result = spawnProcess(command, args, {
      cwd, env: { PATH: process.env.PATH, LANG: 'C' }, timeout: 60_000, encoding: 'utf8',
    })
    if (result.error || result.status !== 0) throw nativeFailure(command, result)
    return String(result.stdout ?? '').trim()
  }
  // Use the public setup from the signed payload. Launch the original AppImage.
  const relative = 'usr/lib/muniment/cef/chrome-sandbox'
  for (const entry of [relative, 'setup-sandbox.sh']) {
    execute(packageFile, ['--appimage-extract', entry], expanded)
  }
  const helper = path.join(expanded, 'squashfs-root', relative)
  const setup = path.join(expanded, 'squashfs-root', 'setup-sandbox.sh')
  for (const file of [helper, setup]) {
    const stat = fs.lstatSync(file)
    if (!stat.isFile() || stat.size === 0) throw new Error('The signed AppImage lacks a regular sandbox setup or helper.')
  }
  // A disposable guest must not overwrite another installation.
  try {
    fs.lstatSync(linuxSandbox)
    throw new Error('The Linux guest already contains a Chromium sandbox helper.')
  } catch (error) { if (error.code !== 'ENOENT') throw error }
  const cleanup = () => execute('sudo', ['-n', 'rm', '-f', '--', linuxSandbox])
  try {
    try {
      execute('sudo', ['-n', '/bin/sh', setup])
      if (execute('stat', ['--format=%u:%g:%a', linuxSandbox]) !== '0:0:4755') throw new Error(sandboxRequirement)
      const options = execute('findmnt', ['--noheadings', '--output', 'OPTIONS', '--target', linuxSandbox]).split(',')
      if (!options[0] || options.includes('nosuid') || options.includes('noexec')) throw new Error(sandboxRequirement)
      execute('cmp', ['--silent', '--', helper, linuxSandbox])
    } finally { fs.rmSync(expanded, { recursive: true, force: true }) }
    return cleanup
  } catch (error) {
    try { cleanup() } catch (cleanupError) {
      throw new Error(`${error.message}\nSandbox cleanup failed: ${cleanupError.message}`)
    }
    throw error
  }
}
