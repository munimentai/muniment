import fs from 'node:fs'
import path from 'node:path'

// Collect only Pi session logs and runtime diagnostics before sign-in starts Pi again.
const { localRoots, profileRoots, destination } = JSON.parse(fs.readFileSync(0, 'utf8'))
const unique = (roots) => [...new Set(roots.filter(Boolean))]
const regularFile = (file) => {
  try { return fs.lstatSync(file).isFile() } catch (error) {
    if (error.code === 'ENOENT') return false
    throw error
  }
}

const stderr = []
for (const root of unique(localRoots)) {
  const file = path.join(root, 'ai.muniment.desktop', 'logs', 'runtime.log')
  if (!regularFile(file)) continue
  stderr.push(`The runtime wrote log ${file}.\n${fs.readFileSync(file, 'utf8')}\n`)
}
fs.writeFileSync(path.join(destination, 'pi-local-mode-stderr.log'),
  stderr.join('') || 'No runtime log exists for the local mode run.\n')

const sessions = []
for (const root of unique(profileRoots)) {
  const directory = path.join(root, 'ai.muniment.desktop', 'pi-sessions')
  let entries
  try { entries = fs.readdirSync(directory).sort() } catch (error) {
    if (error.code === 'ENOENT') continue
    throw error
  }
  for (const entry of entries) {
    const file = path.join(directory, entry)
    if (!entry.endsWith('.jsonl') || !regularFile(file)) continue
    sessions.push(`Pi wrote session log ${entry}.\n${fs.readFileSync(file, 'utf8')}\n`)
  }
}
fs.writeFileSync(path.join(destination, 'pi-local-mode-chat.log'),
  sessions.join('') || 'No Pi session log exists for the local mode run.\n')
