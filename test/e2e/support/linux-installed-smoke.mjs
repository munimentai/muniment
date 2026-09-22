import { createReadStream, mkdirSync, mkdtempSync, openSync, closeSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawn, spawnSync } from 'node:child_process'

const [image, reportDir] = process.argv.slice(2).map(value => resolve(value))
if (!image || !reportDir || process.platform !== 'linux') throw new Error('usage: linux-installed-smoke.mjs <AppImage> <report directory>')
const prerequisite = spawnSync('xdotool', ['--version'], { encoding: 'utf8', timeout: 3000 })
if (prerequisite.error || prerequisite.status !== 0) throw new Error('xdotool is required for installed window verification')
const hash = async () => {
  const digest = createHash('sha256')
  for await (const chunk of createReadStream(image)) digest.update(chunk)
  return digest.digest('hex')
}
const delay = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds))
const groupAlive = pid => { try { process.kill(-pid, 0); return true } catch (error) { if (error.code === 'ESRCH') return false; throw error } }
const work = mkdtempSync(join(tmpdir(), 'muniment-installed-appimage-'))
mkdirSync(reportDir, { recursive: true })
rmSync(join(reportDir, 'appimage-installed.json'), { force: true })
mkdirSync(join(work, 'runtime'), { mode: 0o700 })
mkdirSync(join(work, 'home'), { mode: 0o700 })
const log = join(reportDir, 'appimage-installed.log')
const fd = openSync(log, 'w', 0o600)
const env = { ...process.env, HOME: join(work, 'home'), XDG_RUNTIME_DIR: join(work, 'runtime'), XDG_DATA_HOME: join(work, 'data'),
  XDG_CONFIG_HOME: join(work, 'config'), XDG_CACHE_HOME: join(work, 'cache'), MUNIMENT_STATE_DIR: join(work, 'state'), GDK_BACKEND: 'x11' }
for (const name of ['LD_LIBRARY_PATH', 'APPDIR', 'APPIMAGE']) delete env[name]
const before = await hash()
let app, ready = false, spawnError
try {
  app = spawn(image, [], { detached: true, env, stdio: ['ignore', fd, fd] })
  app.on('error', error => { spawnError = error })
  const deadline = Date.now() + 60000
  while (Date.now() < deadline) {
    if (spawnError) throw spawnError
    if (app.exitCode !== null || app.signalCode !== null) throw new Error('The installed AppImage exited before readiness')
    const windows = spawnSync('xdotool', ['search', '--onlyvisible', '--name', '^muniment$'], { encoding: 'utf8', timeout: 3000 })
    const ownedWindow = (windows.stdout || '').trim().split(/\s+/).filter(Boolean).some(window => {
      const result = spawnSync('xdotool', ['getwindowpid', window], { encoding: 'utf8', timeout: 3000 })
      if (result.status !== 0 || !/^\d+\s*$/.test(result.stdout || '')) return false
      try {
        const stat = readFileSync(`/proc/${result.stdout.trim()}/stat`, 'utf8')
        return Number(stat.slice(stat.lastIndexOf(')') + 2).split(' ')[2]) === app.pid
      } catch { return false }
    })
    if (ownedWindow && readFileSync(log, 'utf8').includes('Desktop client connected: true.')) { ready = true; break }
    await delay(500)
  }
  if (!ready) throw new Error('The installed AppImage did not open a window with a connected runtime')
  if (await hash() !== before) throw new Error('The installed AppImage changed during the smoke test')
} finally {
  try {
    if (app?.pid && groupAlive(app.pid)) {
      process.kill(-app.pid, 'SIGTERM')
      for (let i = 0; i < 40 && groupAlive(app.pid); i++) await delay(250)
      if (groupAlive(app.pid)) process.kill(-app.pid, 'SIGKILL')
      for (let i = 0; i < 20 && groupAlive(app.pid); i++) await delay(250)
      if (groupAlive(app.pid)) throw new Error('The installed AppImage process group did not stop')
    }
  } finally {
    closeSync(fd)
    rmSync(work, { recursive: true, force: true })
  }
}
writeFileSync(join(reportDir, 'appimage-installed.json'), JSON.stringify({ sha256: before,
  unchanged: true, visibleWindow: ready, runtimeConnected: ready }, null, 2), { mode: 0o600 })
