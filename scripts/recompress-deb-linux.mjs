import { existsSync, mkdtempSync, readdirSync, renameSync, rmSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'

// Tauri writes the package data with gzip. dpkg reads xz on every Ubuntu,
// Pop!_OS and Debian release the package supports, and xz makes it about a
// quarter smaller. The control member and the file modes stay as Tauri wrote
// them, because only the data member's compression changes.
export function recompressDeb(deb, run = spawnSync) {
  const execute = (command, args, options = {}) => {
    const result = run(command, args, { encoding: 'utf8', ...options })
    if (result.error || result.status !== 0) throw new Error(`Package command failed: ${command} ${args[0]}`)
    return result.stdout ?? ''
  }
  const members = execute('ar', ['t', deb]).trim().split('\n')
  if (members.join(' ') !== 'debian-binary control.tar.gz data.tar.gz') {
    throw new Error(`Unexpected package members: ${members.join(' ')}`)
  }
  const work = mkdtempSync(join(dirname(deb), '.muniment-deb-'))
  try {
    execute('ar', ['x', resolve(deb)], { cwd: work })
    execute('gzip', ['-d', 'data.tar.gz'], { cwd: work })
    execute('xz', ['-6', '-T0', 'data.tar'], { cwd: work })
    const output = join(work, 'package.deb')
    execute('ar', ['rcD', output, 'debian-binary', 'control.tar.gz', 'data.tar.xz'], { cwd: work })
    renameSync(output, deb)
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const directory = resolve('src-tauri/target/release/bundle/deb')
  const packages = existsSync(directory) ? readdirSync(directory).filter(name => name.endsWith('.deb')) : []
  // A build that names other bundles has no package to recompress.
  if (packages.length > 1) throw new Error('Expected one Debian package to recompress')
  if (packages.length) recompressDeb(join(directory, packages[0]))
}
