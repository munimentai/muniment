import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const root = process.cwd()
const workflow = fs.readFileSync(path.join(root, '.github/workflows/ci.yml'), 'utf8')
const desktopCompile = workflow.slice(workflow.indexOf('  desktop-compile:'), workflow.indexOf('  desktop-build:'))
const guardedPattern = /it\.skipIf\(process\.platform !== 'win32'\)/

describe('Windows-only PR test gate', () => {
  it('keeps all 14 Windows-only declarations in the gated test file', () => {
    const guardedFiles = fs.readdirSync(path.join(root, 'test'), { recursive: true })
      .filter((name) => /\.test\.js$/.test(name))
      .map((name) => path.join('test', name))
      .filter((name) => guardedPattern.test(fs.readFileSync(path.join(root, name), 'utf8')))
      .map((name) => name.replaceAll(path.sep, '/'))

    expect(guardedFiles).toEqual(['test/desktop-e2e-harness.test.js'])
    expect(fs.readFileSync(path.join(root, guardedFiles[0]), 'utf8').match(new RegExp(guardedPattern, 'g'))).toHaveLength(14)
  })

  it('runs the guarded test file in the Windows PR compile lane', () => {
    const windowsCommand = desktopCompile.match(/if \[ "\$PLATFORM" = "windows" \]; then\n\s+cmd='([^']+)'/)?.[1]

    expect(windowsCommand).toContain('npx vitest run --root . test/desktop-e2e-harness.test.js')
    expect(desktopCompile).toContain("if: github.event_name == 'pull_request' && needs.smoke.outputs.desktop == 'true'")
    expect(desktopCompile).toContain('platform: [linux, windows, macos]')
  })
})
