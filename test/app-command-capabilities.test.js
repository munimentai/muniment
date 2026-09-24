import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'

// Every app command reaches a window only through a capability, so each
// window root must be granted every command its import graph names.
const root = path.resolve('.')
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8')
const permission = (command) => `allow-${command.replaceAll('_', '-')}`

const handler = read('src-tauri/src/desktop.rs').match(/generate_handler!\[([\s\S]*?)\]\)/)[1]
const registered = [...handler.matchAll(/^\s*(?:[a-z0-9_]+::)*([a-z0-9_]+),?\s*$/gm)].map((match) => match[1])
const manifest = [...read('src-tauri/build.rs').match(/const APP_COMMANDS: &\[&str\] = &\[([\s\S]*?)\];/)[1].matchAll(/"([a-z0-9_]+)"/g)].map((match) => match[1])
const capability = (name) => JSON.parse(read(`src-tauri/capabilities/${name}.json`))
const e2e = JSON.parse(read('src-tauri/tauri.e2e.conf.json')).app.security.capabilities.find((entry) => entry.identifier === 'e2e-webdriver')

function imports(entry) {
  const seen = new Set()
  const stack = [path.join(root, entry)]
  while (stack.length) {
    const file = stack.pop()
    if (seen.has(file)) continue
    seen.add(file)
    const source = fs.readFileSync(file, 'utf8')
    for (const match of source.matchAll(/(?:from\s*|import\s*\(?\s*)['"](\.[^'"]+)['"]/g)) {
      const base = path.resolve(path.dirname(file), match[1])
      const next = [base, `${base}.js`, `${base}.svelte`].find((candidate) => /\.(js|svelte)$/.test(candidate) && fs.existsSync(candidate))
      if (next) stack.push(next)
    }
  }
  return [...seen]
}

function invoked(entry) {
  const sources = imports(entry).map((file) => fs.readFileSync(file, 'utf8'))
  return registered.filter((command) => sources.some((source) => new RegExp(`['"\`]${command}['"\`]`).test(source)))
}

describe('app command capabilities', () => {
  it('declares one manifest permission for each registered command', () => {
    expect([...manifest].sort()).toEqual([...registered].sort())
  })

  it.each([
    ['default', 'src/App.svelte'],
    ['launcher', 'src/Launcher.svelte'],
    ['workspace-menu', 'src/lib/WorkspacePopup.svelte'],
  ])('grants the %s window every command its interface invokes', (name, entry) => {
    const granted = capability(name).permissions.filter((entry) => typeof entry === 'string' && entry.startsWith('allow-'))
    expect(granted.filter((entry) => !registered.map(permission).includes(entry))).toEqual([])
    expect(invoked(entry).map(permission).filter((entry) => !granted.includes(entry))).toEqual([])
  })

  it('grants the launcher only its own panel commands', () => {
    expect(capability('launcher').permissions.filter((entry) => entry.startsWith('allow-')).sort()).toEqual(
      ['allow-launcher-close', 'allow-launcher-present-main', 'allow-launcher-start-failed'],
    )
    expect(capability('workspace-menu').permissions.some((entry) => entry.startsWith('allow-'))).toBe(false)
  })

  it('keeps test-only commands in the e2e capability', () => {
    for (const command of ['launcher_open', 'launcher_is_visible', 'e2e_drive_folder_dialog', 'e2e_folder_dialog_snapshot']) {
      expect(e2e.permissions).toContain(permission(command))
      expect(capability('default').permissions).not.toContain(permission(command))
    }
  })

  it('grants the runtime notice probe to the main window', () => {
    expect(read('src-tauri/src/macos_runtime_notice_probe.rs')).toContain("invoke('runtime_notice_observed'")
    expect(capability('default').permissions).toContain('allow-runtime-notice-observed')
  })
})
