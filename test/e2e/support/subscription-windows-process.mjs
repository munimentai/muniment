import fs from 'node:fs'
import path from 'node:path'
import { randomUUID } from 'node:crypto'
import { spawn } from 'node:child_process'

const jobs = new WeakMap()
const delay = ms => new Promise(resolve => setTimeout(resolve, ms))

export async function launchWindowsTree(executable, args, env, log, profile) {
  const id = randomUUID()
  const jobName = `Local\\MunimentSubscription-${id}`
  const launchFile = path.join(env.TMPDIR, `subscription-job-${id}.json`)
  if (profile) fs.writeFileSync(path.join(profile, 'subscription-probe-job.json'), JSON.stringify({ name: jobName }), { mode: 0o600 })
  fs.writeFileSync(launchFile, JSON.stringify({ executable, args, jobName }), { mode: 0o600 })
  const child = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File',
    path.resolve('test/e2e/support/subscription-stop.ps1'), '-LaunchFile', launchFile],
  { env, stdio: ['ignore', log, log], windowsHide: true })
  const closed = new Promise(resolve => {
    child.once('close', code => resolve(code))
    child.once('error', error => { child.diagnosticError = error.message; resolve(-1) })
  })
  jobs.set(child, { launchFile, closed })
  const deadline = Date.now() + 30_000
  try {
    while (!fs.existsSync(`${launchFile}.ready`)) {
      if (child.exitCode !== null || child.signalCode !== null || child.diagnosticError) {
        throw new Error('The native probe job could not start.')
      }
      if (Date.now() >= deadline) throw new Error('The native probe job did not start before the deadline.')
      await delay(25)
    }
    child.probePid = Number(fs.readFileSync(`${launchFile}.ready`, 'utf8'))
    if (!Number.isSafeInteger(child.probePid) || child.probePid <= 0 || child.probePid > 2147483647) {
      throw new Error('The native probe process ID is invalid.')
    }
    return child
  } catch (error) {
    await stopWindowsTree(child)
    throw error
  }
}

export function windowsTreeAlive(child) {
  const job = jobs.get(child)
  return Boolean(job && child.exitCode === null && child.signalCode === null &&
    !child.diagnosticError && !fs.existsSync(`${job.launchFile}.exited`))
}

export async function stopWindowsTree(child) {
  const job = jobs.get(child)
  if (!job) throw new Error('The native probe job identity is missing.')
  // Only the launch receipt selects the job. Never reopen a process by its PID.
  fs.writeFileSync(`${job.launchFile}.stop`, '1', { mode: 0o600 })
  let timer
  try {
    const code = await Promise.race([job.closed, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error('The native probe job did not stop before the deadline.')), 20_000)
    })])
    if (code !== 0) throw new Error('The native probe job could not stop every descendant.')
  } finally { clearTimeout(timer) }
}
