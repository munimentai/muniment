import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawn } from 'node:child_process'
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
    const locked = path.join(profile, 'locked')
    const ready = path.join(root, 'holder-ready')
    fs.writeFileSync(holder, `param([string]$Locked, [string]$Ready)
$ErrorActionPreference = 'Stop'
$file = [IO.File]::Open($Locked, 'OpenOrCreate', 'ReadWrite', 'None')
try {
    [IO.File]::WriteAllText($Ready, '1')
    Start-Sleep -Seconds 120
} finally { $file.Dispose() }
`)
    fs.writeFileSync(fixture, `const { spawn } = require('node:child_process')
const fs = require('node:fs')
const path = require('node:path')
const [mode, holder, locked, ready, rootExits] = process.argv.slice(2)
const logFile = path.join(__dirname, 'job.log')
const log = fs.openSync(logFile, 'a')
const stderr = fs.openSync(path.join(__dirname, 'holder-stderr.log'), 'a')
// Detach the Node intermediate. Let PowerShell create a hidden console instead of using DETACHED_PROCESS.
const child = mode === 'root'
  ? spawn(process.execPath, [__filename, 'middle', holder, locked, ready], { detached: true, stdio: ['ignore', log, log] })
  : spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-File', holder, '-Locked', locked, '-Ready', ready], { windowsHide: true, stdio: ['ignore', log, stderr] })
child.once('error', error => fs.appendFileSync(logFile, 'The ' + mode + ' fixture could not start its child.\\n' + error.stack + '\\n'))
fs.appendFileSync(logFile, 'The ' + mode + ' fixture started child pid=' + (child.pid ?? 'none') + '.\\n')
child.unref()
fs.closeSync(log)
fs.closeSync(stderr)
if (mode === 'root' && rootExits === 'false') setInterval(() => {}, 1000)
`)
    const log = fs.openSync(path.join(root, 'job.log'), 'a')
    let job, unrelated, restarted
    try {
      job = await launchWindowsTree(process.execPath, mode === 'restart' ? []
        : [fixture, 'root', holder, locked, ready, String(rootExits)],
      { ...process.env, TMPDIR: root }, log, profile)
      const jobFile = path.join(profile, 'subscription-probe-job.json')
      assert.match(JSON.parse(fs.readFileSync(jobFile)).name, /^Local\\MunimentSubscription-[a-f0-9-]{36}$/)
      if (mode === 'restart') {
        await waitFor(() => !windowsTreeAlive(job))
        const restart = path.join(root, 'restart.ps1')
        fs.writeFileSync(restart, `param([string]$JobFile, [string]$Node, [string]$Fixture, [string]$Holder, [string]$Locked, [string]$Ready)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class JoinProbeJob {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr OpenJobObject(uint access, bool inherit, string name);
    [DllImport("kernel32.dll")]
    public static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll")]
    public static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll")]
    public static extern bool CloseHandle(IntPtr handle);
}
'@
$name = (Get-Content -LiteralPath $JobFile -Raw | ConvertFrom-Json).name
$handle = [JoinProbeJob]::OpenJobObject(1, $false, $name)
if ($handle -eq [IntPtr]::Zero) { throw 'The test job is missing.' }
try {
    if (-not [JoinProbeJob]::AssignProcessToJobObject($handle, [JoinProbeJob]::GetCurrentProcess())) {
        throw 'The test restart could not join its job.'
    }
} finally { $null = [JoinProbeJob]::CloseHandle($handle) }
& $Node $Fixture root $Holder $Locked $Ready true
`)
        // MSI starts outside the tree. Its replacement joins the retained job before it spawns children.
        restarted = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-File', restart,
          '-JobFile', jobFile, '-Node', process.execPath, '-Fixture', fixture,
          '-Holder', holder, '-Locked', locked, '-Ready', ready], { stdio: ['ignore', log, log] })
        restarted.once('error', () => {})
      }
      await waitFor(() => fs.existsSync(ready))
      if (rootExits) await waitFor(() => !windowsTreeAlive(job))
      else assert.equal(windowsTreeAlive(job), true)
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
      const diagnostics = ['job.log', 'holder-stderr.log'].map(name =>
        `${name}:\n${readDiagnosticLog(path.join(root, name), root, redact)}`).join('\n')
      t.diagnostic(diagnostics)
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
