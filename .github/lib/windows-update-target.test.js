import { describe, expect, it } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
const powershell = process.platform === 'win32' ? 'powershell.exe' : 'pwsh';
const available = process.platform === 'win32' || spawnSync(powershell, ['-NoProfile', '-Command', 'exit 0'], { timeout: 15_000 }).status === 0;
const location = 'C:\\Apps\\muniment';
const msi = { InstallLocation: location, WindowsInstaller: 1, DisplayName: 'muniment' };
describe.skipIf(!available)('Windows updater package identity', { timeout: 30_000 }, () => {
  it.each([
    [{ user: [msi], machine: [] }, 'windows-x86_64-msi-user'],
    [{ user: [], machine: [msi] }, 'windows-x86_64-msi-machine'],
    [{ user: [{ InstallLocation: location, DisplayName: 'muniment', UninstallString: 'C:\\Apps\\muniment\\uninstall.exe' }], machine: [] }, 'windows-x86_64-nsis'],
    [{ user: [msi], machine: [msi] }, null],
    [{ user: [{ ...msi, InstallLocation: 'C:\\Other' }], machine: [] }, null],
    [{ user: [], machine: [] }, null],
  ])('selects exactly one registered installer: %j', (entries, expected) => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-update-target-'));
    try {
      fs.writeFileSync(path.join(root, 'entries.json'), JSON.stringify(entries));
      fs.writeFileSync(path.join(root, 'fixture.ps1'), `function Get-ItemProperty { param($Path, $ErrorAction)\n$data = Get-Content -Raw $env:TEST_ENTRIES | ConvertFrom-Json\nif ($Path.StartsWith('HKCU:')) { $data.user } else { $data.machine }\n}\n. $env:TEST_SCRIPT\n`);
      const result = spawnSync(powershell, ['-NoProfile', '-NonInteractive', '-File', path.join(root, 'fixture.ps1')], { timeout: 20_000, encoding: 'utf8', env: { ...process.env, MUNIMENT_UPDATE_INSTALL_DIR: location, TEST_ENTRIES: path.join(root, 'entries.json'), TEST_SCRIPT: path.resolve('scripts/windows-update-target.ps1') } });
      expect(result.error, result.stderr).toBeUndefined();
      if (expected) { expect(result.status, result.stderr).toBe(0); expect(result.stdout.trim()).toBe(expected); }
      else { expect(result.status).not.toBe(0); expect(result.stderr).toContain('Expected exactly one installed Muniment package.'); }
    } finally { fs.rmSync(root, { recursive: true, force: true }); }
  });
});
