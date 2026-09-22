import { describe, expect, it } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
const powershell = process.platform === 'win32' ? 'powershell.exe' : 'pwsh';
const available = spawnSync(powershell, ['-NoProfile', '-Command', 'exit 0']).status === 0;

describe.skipIf(!available)('unmodified Windows installed smoke', () => {
  it.each(['connected', 'exited', 'no-window', 'disconnected', 'modified'])('checks the installed application: %s', (mode) => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-installed-smoke-'));
    try {
      for (const name of ['app.exe', 'app.dll']) fs.writeFileSync(path.join(root, name), 'shipped bytes');
      fs.writeFileSync(path.join(root, 'fixture.ps1'), `
function Start-Process {
  param($FilePath, [switch]$PassThru, $RedirectStandardOutput, $RedirectStandardError)
  $connected = if ($env:TEST_MODE -eq 'disconnected') { 'false' } else { 'true' }
  Set-Content -LiteralPath $RedirectStandardError -Value @("desktop runtime client connected=true", "desktop runtime client connected=$connected")
  if ($env:TEST_MODE -eq 'modified') { Set-Content -LiteralPath (Join-Path $env:TEST_ROOT 'app.dll') -Value 'changed bytes' }
  $handle = if ($env:TEST_MODE -eq 'no-window') { [IntPtr]::Zero } else { [IntPtr]42 }
  $process = [pscustomobject]@{ HasExited = ($env:TEST_MODE -eq 'exited'); ExitCode = 101; MainWindowHandle = $handle; Id = 987654 }
  $process | Add-Member -MemberType ScriptMethod -Name Refresh -Value {}
  $process | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value { param($milliseconds) $true }
  return $process
}
function taskkill.exe {
  if (($args -join ' ') -ne '/PID 987654 /T /F') { throw 'Wrong process tree.' }
  Set-Content -LiteralPath (Join-Path $env:TEST_ROOT 'stopped') -Value 'yes'
}
& $env:TEST_SCRIPT -AppBinary (Join-Path $env:TEST_ROOT 'app.exe') -AppLibrary (Join-Path $env:TEST_ROOT 'app.dll') -Diagnostics (Join-Path $env:TEST_ROOT 'diagnostics') -WaitSeconds 0
if (-not $?) { exit 1 }
`);
      const result = spawnSync(powershell, ['-NoProfile', '-NonInteractive', '-File', path.join(root, 'fixture.ps1')], { encoding: 'utf8', env: { ...process.env, TEST_ROOT: root, TEST_MODE: mode, TEST_SCRIPT: path.resolve('test/e2e/support/windows-installed-smoke.ps1') } });
      if (mode === 'connected') {
        expect(result.status, result.stderr).toBe(0);
        const proof = JSON.parse(fs.readFileSync(path.join(root, 'diagnostics', 'installed-app-smoke.json'), 'utf8').replace(/^\uFEFF/, ''));
        expect(proof).toMatchObject({ application_alive: true, window_present: true, client_connected: true, application_modified: false });
      } else expect(result.status, result.stdout).not.toBe(0);
      expect(fs.existsSync(path.join(root, 'stopped'))).toBe(mode !== 'exited');
    } finally { fs.rmSync(root, { recursive: true, force: true }); }
  });
});

it('checks shipped bytes before replacing the application DLL', () => {
  const runner = fs.readFileSync('test/e2e/runner/windows.ps1', 'utf8');
  const check = runner.indexOf('test/e2e/support/windows-installed-smoke.ps1');
  expect(check).toBeGreaterThan(runner.indexOf('release DLL WebDriver guard failed'));
  expect(check).toBeLessThan(runner.indexOf('Copy-Item -LiteralPath $webdriverBinary'));
});
