import { describe, expect, it } from 'vitest'
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'

const caskPath = 'Casks/muniment-nightly.rb'
const workflow = readFileSync('.github/workflows/nightly.yml', 'utf8')
const bumpScript = '.github/bump-homebrew-cask.mjs'

describe('Homebrew nightly cask', () => {
  it('points to the checksummed nightly app archive', () => {
    const cask = readFileSync(caskPath, 'utf8')
    const version = cask.match(/^  version "([0-9a-f]{40})"$/m)?.[1]

    expect(version).toBeTruthy()
    expect(cask).toMatch(/^  sha256 "[0-9a-f]{64}"$/m)
    expect(cask).toContain(`nightly-#{version}-macos-muniment.app.zip`)
    expect(cask).toContain('app "muniment.app"')
    expect(cask).toContain('xattr -dr com.apple.quarantine /Applications/muniment.app')
  })

  it('bumps both release fields and rejects invalid values', () => {
    const directory = mkdtempSync(join(tmpdir(), 'muniment-cask-'))
    const copy = join(directory, 'muniment-nightly.rb')
    writeFileSync(copy, readFileSync(caskPath))
    const version = 'a'.repeat(40)
    const digest = 'b'.repeat(64)

    const result = spawnSync(process.execPath, [bumpScript, copy, version, digest], { encoding: 'utf8' })
    expect(result.status, result.stderr).toBe(0)
    expect(readFileSync(copy, 'utf8')).toContain(`version "${version}"\n  sha256 "${digest}"`)

    const invalid = spawnSync(process.execPath, [bumpScript, copy, 'nightly', digest], { encoding: 'utf8' })
    expect(invalid.status).not.toBe(0)
    expect(readFileSync(copy, 'utf8')).toContain(`version "${version}"\n  sha256 "${digest}"`)

    writeFileSync(copy, `${readFileSync(copy, 'utf8')}  sha256 "${digest}"\n`)
    const duplicate = spawnSync(process.execPath, [bumpScript, copy, version, digest], { encoding: 'utf8' })
    expect(duplicate.status).not.toBe(0)
  })

  it('updates the cask after the nightly release is finalized', () => {
    const publish = workflow.slice(workflow.indexOf('  publish:'), workflow.indexOf('  linux-e2e:'))
    expect(publish).toContain('gh release download nightly')
    expect(publish).toContain('shasum -a 256')
    expect(publish).toContain('node .github/bump-homebrew-cask.mjs')
    expect(publish).toContain('git pull --rebase origin main')
    expect(publish).toContain('git push origin HEAD:main')
  })
})
