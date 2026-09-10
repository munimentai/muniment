import { afterEach, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const helperPath = path.join(process.cwd(), 'test/windows-msi-registration.ps1')
const helper = fs.readFileSync(helperPath, 'utf8')
const powershell = process.platform === 'win32' ? 'powershell.exe' : 'pwsh'
const hasPowerShell = spawnSync(powershell, ['-NoProfile', '-Command', 'exit 0'], { timeout: 15_000 }).status === 0
const temporary = []
afterEach(() => { for (const directory of temporary.splice(0)) fs.rmSync(directory, { recursive: true, force: true }) })
const invoke = (body, args = []) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-msi-test-'))
  temporary.push(directory)
  const script = path.join(directory, 'test.ps1')
  fs.writeFileSync(script, `$ErrorActionPreference = 'Stop'\nSet-StrictMode -Version Latest\n${helper}\n${body}\n`)
  return spawnSync(powershell, ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script, ...args], {
    encoding: 'utf8', timeout: 15_000,
  })
}
const code = '{12345678-1234-ABCD-EF12-34567890ABCD}'
const packed = '876543214321DCBAFE2143658709BADC'
const sid = 'S-1-5-21-123'
const local = 'C:\\Users\\fixture\\AppData\\Local'
const location = `${local}\\muniment\\`
const installer = 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Installer'
const keys = {
  userProduct: `HKCU:\\Software\\Microsoft\\Installer\\Products\\${packed}`,
  userData: `${installer}\\UserData\\${sid}\\Products\\${packed}`,
  managed: `${installer}\\Managed\\${sid}\\Installer\\Products\\${packed}`,
  machineProduct: `HKLM:\\SOFTWARE\\Classes\\Installer\\Products\\${packed}`,
}
const uninstall = (hive, wow = false) => `${hive}\\Software\\${wow ? 'WOW6432Node\\' : ''}Microsoft\\Windows\\CurrentVersion\\Uninstall\\${code}`
const fixture = (hive = 'HKLM:') => ({
  [keys.userProduct]: {}, [keys.userData]: {},
  [`${keys.userData}\\InstallProperties`]: { InstallLocation: location, DisplayIcon: `${location}muniment.exe` },
  [uninstall(hive)]: { InstallLocation: location },
})
const runFixture = (registry, absent = false) => invoke(`
$fixture = $args[0] | ConvertFrom-Json
function Test-Path { param($LiteralPath) return $null -ne $fixture.PSObject.Properties[$LiteralPath] }
function Get-ItemProperty { param($LiteralPath) return $fixture.PSObject.Properties[$LiteralPath].Value }
# The registry fixture tests scope checks independently of the Windows path API.
function Test-MsiUserLocation($Location, $LocalAppData) { return $Location -eq '${location}' }
$registration = Get-PerUserMsiRegistration '${code}' '${sid}'
Write-PerUserMsiRegistration $registration '${absent ? 'uninstalled' : 'installed'}'
Assert-PerUserMsiRegistration $registration '${local}' ${absent ? '-Absent' : ''}
`, [JSON.stringify(registry)])

describe('Per-user MSI registration checks', { timeout: 30_000 }, () => {
  it('Reads the ProductCode from the package and closes the MSI database.', () => {
    expect(helper).toContain("WHERE ``Property`` = 'ProductCode'")
    expect(helper).toContain('ConvertTo-PackedProductCode $code | Out-Null')
    expect(helper).toContain('@($record, $view, $database, $installer)')
    expect(helper).toContain('FinalReleaseComObject($comObject)')
  })

  it.skipIf(process.platform !== 'win32')('Returns one GUID string from the real MSI COM reader.', () => {
    const result = invoke(`
$package = Join-Path $PSScriptRoot 'product [fixture].msi'
$installer = $database = $view = $null
try {
  $installer = New-Object -ComObject WindowsInstaller.Installer
  $database = $installer.OpenDatabase($package, 3)
  foreach ($sql in @(
    'CREATE TABLE \`Property\` (\`Property\` CHAR(72) NOT NULL, \`Value\` CHAR(0) LOCALIZABLE PRIMARY KEY \`Property\`)',
    'INSERT INTO \`Property\` (\`Property\`, \`Value\`) VALUES (''ProductCode'', ''${code}'')'
  )) {
    $view = $database.OpenView($sql)
    [void]$view.Execute()
    [void]$view.Close()
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($view)
    $view = $null
  }
  [void]$database.Commit()
} finally {
  foreach ($comObject in @($view, $database, $installer)) {
    if ($null -ne $comObject) { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($comObject) }
  }
}
$output = @(Get-MsiProductCode $package)
if ($output.Count -ne 1) { throw "The MSI reader returned $($output.Count) values instead of one." }
if ($output[0] -isnot [string]) { throw 'The MSI reader did not return a string.' }
ConvertTo-PackedProductCode $output[0] | Out-Null
$output[0]
`)
    expect(result.status, result.stdout + result.stderr).toBe(0)
    expect(result.stdout.trim()).toBe(code)
  })

  it.skipIf(!hasPowerShell)('Packs every GUID field in the Windows Installer order.', () => {
    const result = invoke('ConvertTo-PackedProductCode $args[0]', [code.toLowerCase()])
    expect(result.status, result.stderr).toBe(0)
    expect(result.stdout.trim()).toBe(packed)
  })

  it.skipIf(!hasPowerShell).each(['', '12345678-1234-ABCD-EF12-34567890ABCD', `${code}suffix`, '{GG345678-1234-ABCD-EF12-34567890ABCD}', '*'])('Rejects the invalid ProductCode "%s".', (value) => {
    const result = invoke('ConvertTo-PackedProductCode $args[0]', [value])
    expect(result.status).toBe(1)
    expect(result.stderr).toContain('The MSI ProductCode is invalid:')
  })

  it.skipIf(!hasPowerShell).each(['', 'S-1-*', 'S-1-5-21-123\\other'])('Rejects the invalid SID "%s".', (value) => {
    const result = invoke(`Get-PerUserMsiRegistration '${code}' $args[0]`, [value])
    expect(result.status).toBe(1)
    expect(result.stderr).toContain('The installer SID is invalid:')
  })

  it.skipIf(!hasPowerShell).each(['HKCU:', `Registry::HKEY_USERS\\${sid}`, 'HKLM:'])('Accepts per-user scope with the Uninstall entry in %s.', (hive) => {
    const result = runFixture(fixture(hive))
    expect(result.status, result.stderr).toBe(0)
    for (const [name, key] of Object.entries(keys)) {
      expect(result.stdout).toContain(`ProductCode=${code} ${key} present=${['userProduct', 'userData'].includes(name) ? 1 : 0}`)
    }
    expect(result.stdout).toContain(`ProductCode=${code} InstallLocation=${location}`)
    expect(result.stdout).toContain(`hkcu=${hive === 'HKCU:' ? 1 : 0} HKU\\${sid}=${hive.startsWith('Registry::') ? 1 : 0} hklm=${hive === 'HKLM:' ? 1 : 0}`)
  })

  it.skipIf(!hasPowerShell).each(Object.keys(keys))('Rejects the opposite %s state.', (name) => {
    const registry = fixture()
    if (name === 'userProduct' || name === 'userData') delete registry[keys[name]]
    else registry[keys[name]] = {}
    const result = runFixture(registry)
    expect(result.status, result.stdout + result.stderr).toBe(1)
    expect(result.stderr).toContain('Per-user MSI registration failed')
    expect(result.stdout).toContain(keys[name])
  })

  it.skipIf(!hasPowerShell).each(['missing', 'wrong-sid', 'machine-location', 'empty-location', 'missing-location', 'machine-arp', 'wow-machine-arp'])('Rejects the %s scope.', (mode) => {
    const registry = fixture()
    if (mode === 'missing') {
      for (const key of Object.keys(registry)) delete registry[key]
    } else if (mode === 'wrong-sid') {
      registry[keys.userData.replace(sid, 'S-1-5-21-999')] = registry[keys.userData]
      delete registry[keys.userData]
    } else if (mode.endsWith('arp')) {
      registry[uninstall('HKLM:', mode.startsWith('wow'))] = { InstallLocation: 'C:\\Program Files\\muniment\\' }
    } else {
      registry[`${keys.userData}\\InstallProperties`] = mode === 'missing-location' ? {} : {
        InstallLocation: mode === 'empty-location' ? '' : 'C:\\Program Files\\muniment\\',
      }
    }
    const result = runFixture(registry)
    expect(result.status, result.stdout + result.stderr).toBe(1)
    expect(result.stderr).toContain('Per-user MSI registration failed')
  })

  it.skipIf(!hasPowerShell)('Accepts absent product keys after uninstall and prints every absence.', () => {
    const result = runFixture({}, true)
    expect(result.status, result.stderr).toBe(0)
    for (const key of Object.values(keys)) expect(result.stdout).toContain(`ProductCode=${code} ${key} present=0`)
    expect(result.stdout).toContain(`hkcu=0 HKU\\${sid}=0 hklm=0`)
  })

  it.skipIf(!hasPowerShell).each([
    ...Object.values(keys), `${keys.userData}\\InstallProperties`,
    ...['HKCU:', `Registry::HKEY_USERS\\${sid}`, 'HKLM:'].flatMap((hive) => [uninstall(hive), uninstall(hive, true)]),
  ])('Rejects the leftover key %s after uninstall.', (key) => {
    const result = runFixture({ [key]: { InstallLocation: location } }, true)
    expect(result.status, result.stdout + result.stderr).toBe(1)
    expect(result.stderr).toContain('Per-user MSI registration failed')
  })

  it.skipIf(process.platform !== 'win32').each([
    [location, true], [location.toUpperCase(), true], [`${local}\\child\\..\\muniment`, true],
    ['', false], [local, false], [`${local}-other\\muniment`, false],
    [`${local}\\..\\muniment`, false], ['C:\\Program Files\\muniment', false],
    ['C:\\Users\\other\\AppData\\Local\\muniment', false], ['muniment', false],
    ['C:muniment', false], ['\\\\server\\share\\muniment', false],
  ])('Checks the InstallLocation boundary "%s".', (value, accepted) => {
    const result = invoke('Test-MsiUserLocation $args[0] $args[1]', [value, local])
    expect(result.status, result.stderr).toBe(0)
    expect(result.stdout.trim()).toBe(accepted ? 'True' : 'False')
  })
})
