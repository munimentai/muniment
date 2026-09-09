import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const root = process.cwd()
const workflow = fs.readFileSync(path.join(root, '.github/workflows/ci.yml'), 'utf8')
const job = (name, next) => workflow.slice(workflow.indexOf(`  ${name}:\n`), workflow.indexOf(`  ${next}:\n`))
const changes = job('changes', 'smoke')
const smoke = job('smoke', 'attach-fixtures-current')
const desktopCompile = job('desktop-compile', 'desktop-build')
const desktopBuild = workflow.slice(workflow.indexOf('  desktop-build:\n'))

// The paths the nightly cannot prove one merge later without a red run.
const installerPaths = [
  'src-tauri/tauri*.conf.json',
  'src-tauri/windows/*',
  'src-tauri/packaging/*',
  'src-tauri/capabilities/*',
  '.github/build-*.mjs',
  '.github/build-*.sh',
  '.github/upload-nightly-assets.mjs',
  '.github/workflows/ci.yml',
  'test/e2e/*',
]

describe('PR gate shape', () => {
  it('classifies the diff in its own job and exposes the installer flag', () => {
    expect(changes).toContain('installer: ${{ steps.changes.outputs.installer }}')
    expect(changes).toContain('desktop: ${{ steps.changes.outputs.desktop }}')
    expect(changes).toContain('echo "installer=$installer" >> "$GITHUB_OUTPUT"')
    const lines = changes.split('\n')
    const flag = lines.findIndex((line) => line.trim() === 'installer=true')
    expect(flag).toBeGreaterThan(0)
    let patternLine = flag - 1
    while (lines[patternLine].trim().startsWith('#')) patternLine -= 1
    const patterns = lines[patternLine].trim().replace(/\)$/, '').split('|')
    for (const p of installerPaths) expect(patterns).toContain(p)
  })

  it('runs smoke and the preflights off the changes job, not off each other', () => {
    expect(smoke).toContain('    needs: changes\n')
    expect(smoke).not.toContain('Classify PR changes')
    expect(desktopCompile).toContain('    needs: changes\n')
    expect(desktopCompile).toContain("if: github.event_name == 'pull_request' && needs.changes.outputs.desktop == 'true'")
    expect(workflow).not.toContain('needs.smoke.outputs')
  })

  it('builds the three installers on a pull request only for installer paths', () => {
    expect(desktopBuild).toContain("if: github.event_name == 'pull_request' && needs.changes.outputs.desktop == 'true' && needs.changes.outputs.installer == 'true'")
    expect(desktopBuild).toContain('needs: [changes, desktop-compile]')
    expect(desktopBuild).toContain('platform: [linux, windows, macos]')
  })
})
