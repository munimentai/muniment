import { readFileSync } from 'node:fs'
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
})
