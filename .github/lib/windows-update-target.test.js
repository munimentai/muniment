import { describe, expect, it } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
const powershell = process.platform === 'win32' ? 'powershell.exe' : 'pwsh';
const available = process.platform === 'win32' || spawnSync(powershell, ['-NoProfile', '-Command', 'exit 0'], { timeout: 15_000 }).status === 0;
const location = 'C:\\Apps\\muniment';
const productCode = '{12345678-1234-ABCD-EF12-34567890ABCD}';
const msi = { InstallLocation: location, WindowsInstaller: 1, DisplayName: 'muniment', PSChildName: productCode };
const product = (context, fields = {}) => ({ Context: context, State: '5', InstallLocation: location, ...fields });
const fixture = `
$ErrorActionPreference = 'Stop'
$data = Get-Content -Raw $env:TEST_ENTRIES | ConvertFrom-Json
function Get-ItemProperty { param($Path, $ErrorAction)
  if ($Path.StartsWith('HKCU:')) { $data.user } else { $data.machine }
}
# Model the MSI COM property interface at the native boundary.
Add-Type @'
using System.Runtime.CompilerServices;
public class UpdateProduct {
  public int Context { get; set; }
  public string State { get; set; }
  public string InstallLocation { get; set; }
  [IndexerName("InstallProperty")]
  public string this[string name] {
    get {
      if (name == "State") return State;
      if (name == "InstallLocation") return InstallLocation;
      throw new System.Exception("The MSI property is invalid.");
    }
  }
}
'@
function New-Object { param($ComObject)
  if ($ComObject -ne 'WindowsInstaller.Installer') { throw 'The COM object is invalid.' }
  $installer = [PSCustomObject]@{}
  $installer | Add-Member ScriptMethod ProductsEx {
    param($code, $sid, $contexts)
    if ($code -ne '${productCode}' -or $sid -ne '' -or $contexts -ne 7) { throw 'The MSI query is invalid.' }
    if ($data.queryError) { throw 'The MSI query failed.' }
    foreach ($record in $data.products) {
      $product = [UpdateProduct]::new()
      $product.Context = $record.Context
      $product.State = $record.State
      $product.InstallLocation = $record.InstallLocation
      $product
    }
  }
  return $installer
}
. $env:TEST_SCRIPT
`;
function runTarget(entries) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-update-target-'));
  try {
    fs.writeFileSync(path.join(root, 'entries.json'), JSON.stringify(entries));
    fs.writeFileSync(path.join(root, 'fixture.ps1'), fixture);
    return spawnSync(powershell, ['-NoProfile', '-NonInteractive', '-File', path.join(root, 'fixture.ps1')], {
      timeout: 20_000, encoding: 'utf8', env: { ...process.env, MUNIMENT_UPDATE_INSTALL_DIR: location,
        TEST_ENTRIES: path.join(root, 'entries.json'), TEST_SCRIPT: path.resolve('scripts/windows-update-target.ps1') },
    });
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
}
describe.skipIf(!available)('Windows updater package identity', { timeout: 30_000 }, () => {
  it.each([
    [{ user: [msi], machine: [], products: [product(2)] }, 'windows-x86_64-msi-user'],
    [{ user: [], machine: [msi], products: [product(4)] }, 'windows-x86_64-msi-machine'],
    // The installed Windows candidate registers its per-user MSI under HKLM.
    [{ user: [], machine: [msi], products: [product(2)] }, 'windows-x86_64-msi-user'],
    [{ user: [{ InstallLocation: location, DisplayName: 'muniment', UninstallString: 'C:\\Apps\\muniment\\uninstall.exe' }], machine: [] }, 'windows-x86_64-nsis'],
    [{ user: [msi], machine: [msi], products: [product(2)] }, null, 'Expected exactly one installed Muniment package.'],
    [{ user: [{ ...msi, InstallLocation: 'C:\\Other' }], machine: [], products: [product(2)] }, null, 'Expected exactly one installed Muniment package.'],
    [{ user: [], machine: [] }, null, 'Expected exactly one installed Muniment package.'],
    ...[
      [], [product(2), product(4)], [product(2), product(2)], [product(1)], [product(0)], [product(99)],
      [product(2, { State: '1' })], [product(2, { State: 'invalid' })],
      [product(2, { InstallLocation: '' })], [product(2, { InstallLocation: 'C:\\Other' })],
    ].map(products => [{ user: [], machine: [msi], products }, null]),
    [{ user: [], machine: [msi], products: [product(2)], queryError: true }, null],
    ...['', '*', `${productCode}suffix`].map(PSChildName => [{ user: [], machine: [{ ...msi, PSChildName }], products: [product(2)] }, null]),
  ])('selects exactly one registered installer: %j', (entries, expected, error) => {
    const result = runTarget(entries);
    expect(result.error, result.stderr).toBeUndefined();
    if (expected) { expect(result.status, result.stderr).toBe(0); expect(result.stdout.trim()).toBe(expected); }
    else {
      expect(result.status).not.toBe(0);
      expect(result.stdout.trim()).toBe('');
      if (error) expect(result.stderr).toContain(error);
    }
  });
});
