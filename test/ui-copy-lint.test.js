import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'

import { forbiddenEmDashes, forbiddenHarnessCopy, forbiddenUiCopy, lintEmDashes, lintUiCopy } from './ui-copy-lint.mjs'

describe('UI copy lint', () => {
  it('accepts the shipped UI copy', () => {
    expect(['src', 'src-tauri', 'browser-control'].flatMap(lintUiCopy)).toEqual([])
    expect(lintEmDashes(['src', 'src-tauri', 'browser-control'])).toEqual([])
  })

  it.each(['AI', 'magic', 'supercharge', 'unlocked', 'sovereignty', 'Pi', 'pi', 'PI'])(
    'rejects the forbidden word %s',
    (word) => {
      expect(forbiddenUiCopy(`<button>${word}</button>`)).toEqual([
        { file: '<fixture>', line: 1, word },
      ])
    },
  )

  it.each([
    ['src/App.svelte', '<p>Pi answers here.</p>'],
    ['src/App.svelte', '<p>Pi-powered replies.</p>'],
    ['src/status.js', 'const copy = "Ask Pi."'],
    ['src/status.js', 'const copy = "...Pi answers."'],
    ['src/App.svelte', `<p>{ready ? "Pi's settings are ready." : 'Wait.'}</p>`],
    ['src/status.js', 'const copy = `Pi saved the ${provider} key.`'],
    ['src/status.ts', 'export const copy = "Pi failed."'],
    ['src-tauri/src/local_mode.rs', 'return Err("Pi credentials could not be read.".into());'],
    ['src-tauri/src/status.rs', 'const COPY: &str = r#"Pi failed."#;'],
    ['src-tauri/core/src/chat_coordinate.rs', 'fail_start(\n  "Pi did not start. Try again.",\n);'],
    ['src-tauri/core/src/sidecar/pi_chat.rs', 'const COPY: &str = "Pi failed. Try again.";'],
  ])('rejects harness prose in %s', (file, source) => {
    expect(forbiddenHarnessCopy(source, file)).toEqual([
      { file, line: source.startsWith('fail_start') ? 2 : 1, word: 'Pi' },
    ])
  })

  it.each([
    '// Pi starts here.\nconst copy = "Ready."',
    '/* Pi starts here. */ const copy = "Ready."',
    '<!-- Pi starts here. --><p>Ready.</p>',
    'console.info("Pi started.")',
    'const record = "event=pi_start harness=Pi";',
    'eprintln!(\n  "event=pi_start Pi started: {}",\n  detail("Pi")\n);',
    'tracing::info!(event = "pi_start", "Pi started.");',
    'const phase = "acquiring-pi"; const path = "/home/test/.pi/agent";',
    '',
  ])('accepts diagnostics and identifiers: %s', (source) => {
    expect(forbiddenHarnessCopy(source, 'src/status.js')).toEqual([])
  })

  it('does not let a log hide nearby UI copy', () => {
    const source = 'console.log("event=pi_start Pi started."); const copy = "Pi failed."'
    expect(forbiddenHarnessCopy(source, 'src/status.js')).toEqual([
      { file: 'src/status.js', line: 1, word: 'Pi' },
    ])
    expect(forbiddenHarnessCopy('const copy = "Pi failed. console.log(try again)"', 'src/status.js')).toHaveLength(1)
    expect(forbiddenHarnessCopy('const record = "event=pi_start harness=Pi"; const copy = "Pi failed."', 'src/status.js')).toHaveLength(1)
  })

  it('keeps diagnostic exceptions local to their runtime module', () => {
    const source = 'return Err("Pi session binding failed".into());'
    expect(forbiddenHarnessCopy(source, 'src-tauri/core/src/sidecar/pi_chat.rs')).toEqual([])
    expect(forbiddenHarnessCopy(source, 'src-tauri/src/local_mode.rs')).toHaveLength(1)
  })

  it('accepts the scan registry without exempting other Rust UI copy', () => {
    const file = 'src-tauri/core/src/harness_scan.rs'
    expect(forbiddenHarnessCopy(fs.readFileSync(file, 'utf8'), file)).toEqual([])
    expect(forbiddenHarnessCopy('const COPY: &str = "Pi";', 'src-tauri/src/status.rs')).toHaveLength(1)
  })

  it('checks every CLI root and returns failure for Rust UI copy', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-copy-lint-'))
    const frontend = path.join(root, 'src')
    const backend = path.join(root, 'src-tauri')
    fs.mkdirSync(frontend)
    fs.mkdirSync(backend)
    const fixture = path.join(backend, 'status.rs')
    const run = () => spawnSync(process.execPath, ['test/ui-copy-lint.mjs', frontend, backend], { encoding: 'utf8' })
    try {
      fs.writeFileSync(fixture, 'eprintln!("event=pi_start Pi started.");')
      expect(run().status).toBe(0)
      fs.appendFileSync(fixture, '\nreturn Err("Pi failed.".into());')
      const failure = run()
      expect(failure.status).toBe(1)
      expect(failure.stderr).toContain('status.rs:2: forbidden UI copy: Pi')
    } finally {
      fs.rmSync(root, { recursive: true, force: true })
    }
  })

  it('does not reject a word that only contains the same letters', () => {
    expect(forbiddenUiCopy('<p>Mail is available.</p>')).toEqual([])
  })

  it.each([
    String.fromCodePoint(0x2014),
    '&mdash;',
    '&#8212;',
    '&#x2014;',
    String.raw`\u2014`,
    String.raw`\u{2014}`,
  ])('rejects an em dash written as %s', (emDash) => {
    expect(forbiddenEmDashes(`First clause${emDash}second clause.`)).toEqual([
      { file: '<fixture>', line: 1, word: emDash },
    ])
  })

  it('rejects an escaped em dash in a nested template', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-copy-lint-'))
    const nested = path.join(root, 'screens', 'mobile')
    fs.mkdirSync(nested, { recursive: true })
    const fixture = path.join(nested, 'template.js')
    fs.writeFileSync(fixture, String.raw`export const copy = "first\u2014second"`)

    try {
      expect(lintEmDashes([root])).toEqual([
        { file: fixture, line: 1, word: String.raw`\u2014` },
      ])
    } finally {
      fs.rmSync(root, { recursive: true, force: true })
    }
  })

  it('rejects an em dash in an owner upload', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-copy-lint-'))
    const uploads = path.join(root, 'docs', 'mockups', 'mobile', 'uploads')
    fs.mkdirSync(uploads, { recursive: true })
    const fixture = path.join(uploads, 'owner.html')
    fs.writeFileSync(fixture, `First clause${String.fromCodePoint(0x2014)}second clause.`)

    try {
      expect(lintEmDashes([root])).toEqual([
        { file: fixture, line: 1, word: String.fromCodePoint(0x2014) },
      ])
    } finally {
      fs.rmSync(root, { recursive: true, force: true })
    }
  })
})
