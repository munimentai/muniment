import fs from 'node:fs'
import path from 'node:path'
import { readDiagnosticLog } from './subscription-diagnostics.mjs'

const codes = new Set(['started', 'profile-missing', 'job-read-failed', 'job-name-invalid', 'job-open-failed',
  'job-assign-failed', 'observer-timed-out', 'diagnostic-write-failed', 'profile-restored', 'runtime-waiting', 'runtime-connected', 'runtime-failed'])
const pidValid = pid => Number.isInteger(pid) && pid > 0 && pid <= 2147483647
const exitValid = code => code === null || Number.isInteger(code) && code >= 0 && code <= 0xffffffff
const actions = new Set(['LaunchApplication', 'InstallInitialize', 'InstallFinalize', 'InstallValidate',
  'InstallFiles', 'RemoveFiles', 'FindRelatedProducts', 'CostInitialize', 'FileCost', 'CostFinalize',
  'RegisterProduct', 'PublishProduct', 'RegisterUser', 'RemoveExistingProducts', 'InstallExecute', 'InstallExecuteAgain'])

export function windowsStartup(profile) {
  if (!profile) return []
  return readDiagnosticLog(path.join(profile, 'subscription-probe-startup.log'), profile, value => value).split('\n').flatMap(line => {
    const match = /^pid=([1-9][0-9]{0,9}) code=([a-z-]+)$/.exec(line.trim())
    const pid = Number(match?.[1])
    return match && pidValid(pid) && codes.has(match[2]) ? [{ pid, code: match[2] }] : []
  })
}

// Recovery waits for the MSI-launched app. The runner never substitutes its own app launch.
export function updateRuntimeRecovery({ parentPid, startup, appAlive, runtimeAlive, recovered }) {
  if (!pidValid(parentPid) || appAlive !== false || runtimeAlive !== false || typeof recovered !== 'boolean' ||
      !Array.isArray(startup) || !startup.some(row => pidValid(row?.pid) && row.pid !== parentPid && row.code === 'profile-restored')) return false
  if (recovered) throw new Error('The restored probe runtime exited.')
  return true
}

export function windowsUpdateDiagnostics(profile, parentPid) {
  const startup = windowsStartup(profile)
  const started = startup.filter(row => row.code === 'started').map(row => row.pid)
  const pids = [...new Set([...(pidValid(parentPid) ? [parentPid] : []), ...started])]
  const processes = pids.map(pid => {
    let receipt
    try {
      receipt = JSON.parse(readDiagnosticLog(path.join(profile, `subscription-probe-process-${pid}.json`), profile, value => value))
    } catch { /* An absent or partial receipt cannot prove an exit code. */ }
    const valid = receipt?.pid === pid && typeof receipt.cleanup === 'boolean' &&
      exitValid(receipt.exit_code)
    return { pid, observed: Boolean(valid), exit_code: valid ? receipt.exit_code : null, cleanup: valid ? receipt.cleanup : false }
  })
  const observer = readDiagnosticLog(path.join(profile, 'subscription-probe-observer-error'), profile, value => value).trim()
  return { parent_pid: pidValid(parentPid) ? parentPid : null,
    relaunch_started: pidValid(parentPid) && started.some(pid => pid !== parentPid), processes, startup,
    observer_error: observer === 'process-observe-failed' ? observer : null }
}

// Verbose MSI logs include environment values and user paths. Export only fixed action and result fields.
export function safeMsiLog(text) {
  const result = []
  for (const line of text.split(/\r?\n/)) {
    let match = /^(?:MSI \([cs]\) \([0-9A-F:]+\) \[[0-9:.]+\]: )?Action (start|ended) ([0-9:]+): ([A-Za-z][A-Za-z0-9_.]*)(?:\. Return value ([0-9]{1,10})\.|\.)$/.exec(line)
    if (match && (!match[4] || Number(match[4]) <= 0xffffffff)) result.push(`action=${actions.has(match[3]) ? match[3] : 'other'} state=${match[1]}${match[4] ? ` result=${Number(match[4])}` : ''}`)
    match = /^MSI \([cs]\) \([0-9A-F:]+\) \[[0-9:.]+\]: MainEngineThread is returning ([0-9]{1,10})$/.exec(line)
    if (match && Number(match[1]) <= 0xffffffff) result.push(`msiexec_result=${Number(match[1])}`)
  }
  return result.length ? result.join('\n') + '\n' : 'No MSI action or result record exists.\n'
}

export function collectWindowsUpdate(profile, output, parentPid) {
  if (!profile) return
  const diagnostics = windowsUpdateDiagnostics(profile, parentPid)
  fs.writeFileSync(path.join(output, 'windows-subscription-relaunch.json'), JSON.stringify(diagnostics, null, 2) + '\n', { mode: 0o600 })
  // The raw verbose log stays inside the disposable profile until cleanup.
  const text = readDiagnosticLog(path.join(profile, 'subscription-probe-msi.log'), profile, value => value, 4 * 1024 * 1024)
  fs.writeFileSync(path.join(output, 'windows-subscription-msi.log'), safeMsiLog(text), { mode: 0o600 })
}

export function copyWindowsUpdate(input, output) {
  const read = name => readDiagnosticLog(path.join(input, name), input, value => value, 4 * 1024 * 1024)
  try {
    const value = JSON.parse(read('windows-subscription-relaunch.json'))
    const parent = pidValid(value?.parent_pid) ? value.parent_pid : null
    const startup = (Array.isArray(value?.startup) ? value.startup : []).filter(row => pidValid(row?.pid) && codes.has(row.code))
      .map(({ pid, code }) => ({ pid, code }))
    const processes = (Array.isArray(value?.processes) ? value.processes : []).filter(row => pidValid(row?.pid) &&
      typeof row.observed === 'boolean' && exitValid(row.exit_code) && typeof row.cleanup === 'boolean')
      .map(({ pid, observed, exit_code, cleanup }) => ({ pid, observed, exit_code, cleanup }))
    const safe = { parent_pid: parent, relaunch_started: parent !== null && startup.some(row => row.pid !== parent && row.code === 'started'),
      startup, processes, observer_error: value?.observer_error === 'process-observe-failed' ? value.observer_error : null }
    fs.writeFileSync(path.join(output, 'windows-subscription-relaunch.json'), JSON.stringify(safe, null, 2) + '\n', { mode: 0o600 })
  } catch { /* Do not copy malformed diagnostics or parser excerpts. */ }
  const records = read('windows-subscription-msi.log').split('\n').flatMap(line => {
    const action = /^action=([A-Za-z][A-Za-z0-9_.]*) state=(start|ended)(?: result=([0-9]{1,10}))?$/.exec(line)
    if (action && (!action[3] || exitValid(Number(action[3])))) {
      return [`action=${actions.has(action[1]) ? action[1] : 'other'} state=${action[2]}${action[3] ? ` result=${Number(action[3])}` : ''}`]
    }
    const result = /^msiexec_result=([0-9]{1,10})$/.exec(line)
    return result && exitValid(Number(result[1])) ? [`msiexec_result=${Number(result[1])}`] : []
  })
  fs.writeFileSync(path.join(output, 'windows-subscription-msi.log'), records.length ? records.join('\n') + '\n' : 'No MSI action or result record exists.\n', { mode: 0o600 })
}
