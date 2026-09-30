import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawn, spawnSync } from 'node:child_process'
import { launchWindowsTree, stopWindowsTree, windowsTreeAlive } from './e2e/support/subscription-windows-process.mjs'
import { removeProbeProfile } from './e2e/runner/subscriptions.mjs'
import { readDiagnosticLog, subscriptionRedactor } from './e2e/support/subscription-diagnostics.mjs'

const waitFor = async condition => {
  const deadline = Date.now() + 15_000
  while (!condition()) {
    assert.ok(Date.now() < deadline, 'The fixture did not reach its expected state.')
    await new Promise(resolve => setTimeout(resolve, 25))
  }
}

for (const mode of ['live', 'exited', 'restart']) {
  const rootExits = mode !== 'live'
  test(`Windows jobs retain descendants after the intermediate exits in ${mode} mode`, {
    skip: process.platform !== 'win32', timeout: 90_000,
  }, async t => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription job test-'))
    const profile = path.join(root, 'profile')
    fs.mkdirSync(profile)
    const fixture = path.join(root, 'fixture.cjs')
    const holder = path.join(root, 'holder.ps1')
    const ready = path.join(root, 'holder-ready')
    fs.writeFileSync(holder, `$ErrorActionPreference = 'Stop'
$file = $null
try {
    $file = [IO.File]::Open((Join-Path $PSScriptRoot 'profile/locked'), 'OpenOrCreate', 'ReadWrite', 'None')
    [IO.File]::WriteAllText((Join-Path $PSScriptRoot 'holder-ready'), '1')
    Start-Sleep -Seconds 120
} catch {
    [IO.File]::WriteAllText((Join-Path $PSScriptRoot 'holder-stderr.log'), ($_ | Out-String))
    throw
} finally { if ($file) { $file.Dispose() } }
`)
    // Libuv kills non-detached children when their Node parent exits.
    // Native creation leaves libuv's job, but the subscription job still retains the holder.
    const holderLauncher = path.join(root, 'holder-launch.ps1')
    fs.writeFileSync(holderLauncher, `$ErrorActionPreference = 'Stop'
$start = [Diagnostics.ProcessStartInfo]::new()
$start.FileName = Join-Path $PSHOME 'powershell.exe'
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.Arguments = '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + (Join-Path $PSScriptRoot 'holder.ps1') + '"'
$child = [Diagnostics.Process]::Start($start)
try {
    $receipt = Join-Path $PSScriptRoot 'holder-pid'
    [IO.File]::WriteAllText("$receipt.tmp", [string]$child.Id)
    [IO.File]::Move("$receipt.tmp", $receipt)
} finally { $child.Dispose() }
`)
    fs.writeFileSync(fixture, `const { spawn } = require('node:child_process')
const fs = require('node:fs')
const path = require('node:path')
const [mode, rootExits] = process.argv.slice(2)
const logFile = path.join(__dirname, 'job.log')
const log = fs.openSync(logFile, 'a')
const child = mode === 'root'
  ? spawn(process.execPath, [__filename, 'middle'], { detached: true, stdio: ['ignore', log, log] })
  : spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(__dirname, 'holder-launch.ps1')], { windowsHide: true, stdio: ['ignore', log, log] })
child.once('error', error => { fs.appendFileSync(logFile, error.stack + '\\n'); process.exitCode = 1 })
fs.appendFileSync(logFile, 'The ' + mode + ' fixture started child pid=' + (child.pid ?? 'none') + '.\\n')
fs.closeSync(log)
if (mode === 'root') {
  if (child.pid) {
    const receipt = path.join(__dirname, 'middle-pid')
    fs.writeFileSync(receipt + '.tmp', String(child.pid))
    fs.renameSync(receipt + '.tmp', receipt)
  }
  child.unref()
  if (rootExits === 'false') setInterval(() => {}, 1000)
}
// Keep the middle alive until the native launcher exits, not until the holder exits.
`)
    const stateScript = path.join(root, 'state.ps1')
    fs.writeFileSync(stateScript, `$ErrorActionPreference = 'Stop'
$result = @{}
foreach ($name in @('middle', 'holder')) {
    $receipt = Join-Path $PSScriptRoot "$name-pid"
    $state = 'not-started'
    if ([IO.File]::Exists($receipt)) {
        $process = $null
        try {
            $process = [Diagnostics.Process]::GetProcessById([int][IO.File]::ReadAllText($receipt))
            if ($process.HasExited) { $state = 'exited' }
            else { $state = 'running' }
        } catch [ArgumentException] { $state = 'exited' }
        finally { if ($process) { $process.Dispose() } }
    }
    $result[$name] = $state
}
$result | ConvertTo-Json -Compress
`)
    const state = () => {
      const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', stateScript],
        { encoding: 'utf8', timeout: 10_000, windowsHide: true })
      assert.equal(result.status, 0, `The fixture state check failed. ${result.error?.message ?? result.stderr}`)
      return JSON.parse(result.stdout.trim())
    }
    const log = fs.openSync(path.join(root, 'job.log'), 'a')
    let job, unrelated, restarted
    try {
      job = await launchWindowsTree(process.execPath, mode === 'restart' ? [] : [fixture, 'root', String(rootExits)],
      { ...process.env, TMPDIR: root }, log, profile)
      const jobFile = path.join(profile, 'subscription-probe-job.json')
      assert.match(JSON.parse(fs.readFileSync(jobFile)).name, /^Local\\MunimentSubscription-[a-f0-9-]{36}$/)
      if (mode === 'restart') {
        await waitFor(() => !windowsTreeAlive(job))
        const restart = path.join(root, 'restart.ps1')
        fs.writeFileSync(restart, `param([string]$JobFile, [string]$Node, [string]$Fixture)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class JoinProbeJob {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr OpenJobObject(uint access, bool inherit, string name);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool member);
    [DllImport("kernel32.dll")]
    static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll")]
    static extern bool CloseHandle(IntPtr handle);
    public static void Join(string name) {
        IntPtr process = GetCurrentProcess();
        bool member;
        if (!IsProcessInJob(process, IntPtr.Zero, out member))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "The test restart could not check its job membership.");
        Console.WriteLine("The test restart has an inherited job: " + member + ".");
        IntPtr handle = OpenJobObject(1, false, name);
        if (handle == IntPtr.Zero) {
            int error = Marshal.GetLastWin32Error();
            throw new Win32Exception(error, "OpenJobObject failed. GetLastError=" + error + ".");
        }
        try {
            if (!AssignProcessToJobObject(handle, process)) {
                int error = Marshal.GetLastWin32Error();
                throw new Win32Exception(error, "AssignProcessToJobObject failed. GetLastError=" + error + ".");
            }
        } finally { CloseHandle(handle); }
    }
}
'@
$name = (Get-Content -LiteralPath $JobFile -Raw | ConvertFrom-Json).name
try {
    [JoinProbeJob]::Join($name)
    & $Node $Fixture root true
    if ($LASTEXITCODE -ne 0) { throw 'The test restart fixture failed.' }
} catch {
    [IO.File]::WriteAllText((Join-Path $PSScriptRoot 'restart-stderr.log'), ($_ | Out-String))
    throw
}
`)
        // Native creation leaves libuv's silent-breakaway job but preserves the SSH/Actions job chain.
        const launcher = path.join(root, 'restart-launch.ps1')
        fs.writeFileSync(launcher, `param([string]$Node)
$ErrorActionPreference = 'Stop'
$start = [Diagnostics.ProcessStartInfo]::new()
$start.FileName = Join-Path $PSHOME 'powershell.exe'
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.Arguments = '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + (Join-Path $PSScriptRoot 'restart.ps1') +
    '" -JobFile "' + (Join-Path $PSScriptRoot 'profile/subscription-probe-job.json') +
    '" -Node "' + $Node + '" -Fixture "' + (Join-Path $PSScriptRoot 'fixture.cjs') + '"'
$child = [Diagnostics.Process]::Start($start)
try {
    $child.WaitForExit()
    exit $child.ExitCode
} finally { $child.Dispose() }
`)
        // MSI starts outside the tree. Its replacement joins the retained job before it spawns children.
        restarted = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', launcher, '-Node', process.execPath],
          { windowsHide: true, stdio: ['ignore', log, log] })
        restarted.once('error', () => {})
      }
      await waitFor(() => fs.existsSync(ready))
      await waitFor(() => state().middle === 'exited')
      assert.equal(state().holder, 'running')
      if (rootExits) await waitFor(() => !windowsTreeAlive(job))
      else assert.equal(windowsTreeAlive(job), true)
      if (restarted) {
        await waitFor(() => restarted.exitCode !== null)
        assert.equal(restarted.exitCode, 0)
        assert.equal(state().holder, 'running')
      }
      assert.throws(() => fs.rmSync(profile, { recursive: true, force: true }), /EPERM|EBUSY|EACCES/)
      unrelated = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' })
      await new Promise((resolve, reject) => { unrelated.once('spawn', resolve); unrelated.once('error', reject) })
      // Simulate PID reuse. Shutdown must use the retained job, not the reported root PID.
      job.probePid = unrelated.pid
      job.pid = unrelated.pid
      await stopWindowsTree(job)
      assert.equal(unrelated.exitCode, null)
      assert.doesNotThrow(() => process.kill(unrelated.pid, 0))
      await removeProbeProfile(profile)
      assert.equal(fs.existsSync(profile), false)
      await stopWindowsTree(job)
    } catch (error) {
      const redact = subscriptionRedactor()
      try { t.diagnostic(`The fixture process states are ${JSON.stringify(state())}.`) }
      catch (stateError) { t.diagnostic(redact(stateError.message)) }
      if (restarted) t.diagnostic(`The restart exit code is ${restarted.exitCode}.`)
      for (const name of ['job.log', 'holder-stderr.log', 'restart-stderr.log']) {
        t.diagnostic(`${name}:\n${readDiagnosticLog(path.join(root, name), root, redact)}`)
      }
      throw error
    } finally {
      try { if (job) await stopWindowsTree(job) }
      finally {
        try {
          if (restarted?.pid && restarted.exitCode === null) {
            const closed = new Promise(resolve => restarted.once('close', resolve))
            restarted.kill()
            await closed
          }
          if (unrelated?.exitCode === null) {
            const closed = new Promise(resolve => unrelated.once('close', resolve))
            unrelated.kill()
            await closed
          }
        } finally {
          fs.closeSync(log)
          await removeProbeProfile(root)
        }
      }
    }
  })
}
