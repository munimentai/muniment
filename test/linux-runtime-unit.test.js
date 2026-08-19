import { existsSync, lstatSync, mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { describe, expect, it } from 'vitest'

const configPath = 'src-tauri/tauri.linux.conf.json'
const unitPath = 'src-tauri/packaging/muniment-runtime.service'
const postInstallPath = 'src-tauri/packaging/deb/postinst'
const postRemovePath = 'src-tauri/packaging/deb/postrm'

const config = JSON.parse(readFileSync(configPath, 'utf8'))
const deb = config.bundle.linux.deb
const unit = readFileSync(unitPath, 'utf8')
const postInstall = readFileSync(postInstallPath, 'utf8')
const postRemove = readFileSync(postRemovePath, 'utf8')

// systemd reads the two start-limit directives from [Unit] and RestartSec from
// [Service], so each assertion below reads only the section that owns it.
function section(name) {
  const afterHeader = unit.split(`[${name}]\n`)[1] ?? ''
  return afterHeader.split('\n[')[0]
}

describe('Linux runtime user unit', () => {
  it('ships the unit at the systemd user unit path', () => {
    expect(deb.files).toEqual({
      '/usr/lib/systemd/user/muniment-runtime.service': 'packaging/muniment-runtime.service',
    })
    expect(unit).toContain('ExecStart=/usr/lib/muniment/muniment-runtime')
    expect(unit).toContain('Restart=on-failure')
  })

  it('states its restart delay and its start limit', () => {
    expect(section('Service')).toMatch(/^RestartSec=5s$/m)
    expect(section('Unit')).toMatch(/^StartLimitIntervalSec=300s$/m)
    expect(section('Unit')).toMatch(/^StartLimitBurst=5$/m)
  })

  it('starts at login and removes package-owned enablement', () => {
    expect(deb.postInstallScript).toBe('packaging/deb/postinst')
    expect(postInstall).toContain('/etc/systemd/user/default.target.wants/muniment-runtime.service')
    expect(postInstall).toContain('ln -sfn /usr/lib/systemd/user/muniment-runtime.service')

    expect(deb.postRemoveScript).toBe('packaging/deb/postrm')
    expect(postRemove).toContain('rm -f /etc/systemd/user/default.target.wants/muniment-runtime.service')
    expect(unit).toContain('WantedBy=default.target')
  })

  it('points the product-name desktop path at the shipped payload', () => {
    expect(postInstall).toContain('ln -sfn muniment-desktop /usr/bin/muniment')
    expect(postInstall).toContain('[ -e /usr/bin/muniment-desktop ]')
    expect(postInstall).toContain('[ ! -e /usr/bin/muniment ] || [ -L /usr/bin/muniment ]')
    expect(postRemove).toContain('[ -L /usr/bin/muniment ]')
    expect(postRemove).toContain('rm -f /usr/bin/muniment')
  })

  it.skipIf(process.platform === 'win32')('creates that path only when the payload exists and the name is free', () => {
    const root = mkdtempSync(path.join(tmpdir(), 'muniment-deb-'))
    const bin = path.join(root, 'usr', 'bin')
    mkdirSync(bin, { recursive: true })
    const rewrite = (script) => script.replaceAll('/usr/bin/', `${bin}/`).replaceAll('/etc/systemd/user/default.target.wants', path.join(root, 'wants'))
    const run = (script, arg) => {
      const file = path.join(root, path.basename(script))
      writeFileSync(file, rewrite(readFileSync(script, 'utf8')), { mode: 0o700 })
      return spawnSync('sh', [file, arg], { encoding: 'utf8' })
    }

    try {
      const missingPayload = run(postInstallPath, 'configure')
      expect(missingPayload.status, missingPayload.stderr).toBe(0)
      expect(existsSync(path.join(bin, 'muniment'))).toBe(false)

      writeFileSync(path.join(bin, 'muniment-desktop'), 'payload')
      const created = run(postInstallPath, 'configure')
      expect(created.status, created.stderr).toBe(0)
      expect(lstatSync(path.join(bin, 'muniment')).isSymbolicLink()).toBe(true)
      expect(readFileSync(path.join(bin, 'muniment'), 'utf8')).toBe('payload')

      rmSync(path.join(bin, 'muniment'))
      writeFileSync(path.join(bin, 'muniment'), 'real binary')
      const preserved = run(postInstallPath, 'configure')
      expect(preserved.status, preserved.stderr).toBe(0)
      expect(lstatSync(path.join(bin, 'muniment')).isSymbolicLink()).toBe(false)
      expect(readFileSync(path.join(bin, 'muniment'), 'utf8')).toBe('real binary')

      const leftRealFile = run(postRemovePath, 'remove')
      expect(leftRealFile.status, leftRealFile.stderr).toBe(0)
      expect(readFileSync(path.join(bin, 'muniment'), 'utf8')).toBe('real binary')

      rmSync(path.join(bin, 'muniment'))
      symlinkSync('elsewhere', path.join(bin, 'muniment'))
      const replaced = run(postInstallPath, 'configure')
      expect(replaced.status, replaced.stderr).toBe(0)
      expect(lstatSync(path.join(bin, 'muniment')).isSymbolicLink()).toBe(true)
      expect(readFileSync(path.join(bin, 'muniment'), 'utf8')).toBe('payload')

      const removed = run(postRemovePath, 'remove')
      expect(removed.status, removed.stderr).toBe(0)
      expect(existsSync(path.join(bin, 'muniment'))).toBe(false)
      expect(readFileSync(path.join(bin, 'muniment-desktop'), 'utf8')).toBe('payload')
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })
})
