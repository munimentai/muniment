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
    ['src/App.svelte', '<p>See https://example.com for Pi settings.</p>'],
    ['src/App.svelte', '<script>// Ready.</script><p>Pi answers here.</p>'],
    ['src/App.svelte', '<p>{ready ? { label: "Ready." }.label : "Wait."} See https://example.com for Pi settings.</p>'],
    ['src/App.svelte', '<p>// Pi answers here.</p>'],
    ['src/App.svelte', '<p>/* Pi answers here. */</p>'],
    ['src/App.svelte', '<p>console.log(Pi answers here.)</p>'],
    ['src/App.svelte', '<p>See https://example.com</p><p>Pi answers here.</p>'],
    ['src/App.jsx', '<p>Pi answers here.</p>'],
    ['src/App.js', 'const App = () => <p>Pi answers here.</p>'],
    ['src/App.tsx', 'export const App = () => <p>See https://example.com for Pi settings.</p>'],
    ['src/App.jsx', 'const App = () => <><p>Ready.</p><p>Pi answers here.</p></>'],
    ['src/App.jsx', 'const App = () => <div>{ready && <p>Pi answers here.</p>}</div>'],
    ['src/App.jsx', 'const App = () => <p>{/* Ready. */}Pi answers here.</p>'],
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
    'count < 2 ? "Ready" : "Wait"',
    'count > 2 ? "Ready" : "Wait"',
    'count < 2 ? { label: "Ready" }.label : "Wait"',
    'count < 2 ? "}>" : "<{"',
    'count < 2 /* }> Pi stays in this comment. */ ? "Ready" : "Wait"',
    'count < 2 // }> Pi stays in this comment.\n ? "Ready" : "Wait"',
    '() => { return count < 2 ? `Ready ${count}` : "Wait" }',
  ])('balances the attribute expression %s before rendered text', (expression) => {
    for (const file of ['src/App.jsx', 'src/App.tsx', 'src/App.js', 'src/App.svelte']) {
      for (const text of ['Pi answers here.', 'See https://example.com for Pi settings.']) {
        const source = `const App = () => <p title={${expression}}>\n${text}</p>;`
        expect(forbiddenHarnessCopy(source, file)).toEqual([
          { file, line: source.split('\n').length, word: 'Pi' },
        ])
      }
      const source = `const App = () => <p title={${expression}}>Ready.</p>;`
      expect(forbiddenHarnessCopy(source, file)).toEqual([])
    }
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

  it.each([
    ['<fixture>', '// Pi starts here.\nconst copy = "Ready."'],
    ['src/App.svelte', '<script>// Pi starts here.\nconsole.info("Pi started.")</script><p>Ready.</p>'],
    ['src/App.svelte', '<!-- See https://example.com for Pi settings. --><p>Ready.</p>'],
    ['src/App.jsx', '// Pi starts here.\nconst App = () => <p>Ready.</p>'],
    ['src/App.tsx', 'const App = () => <p>{/* Pi starts here. */}Ready.</p>'],
    ['src/App.jsx', 'console.info("Pi started."); const App = () => <p>Ready.</p>'],
    ['src/App.jsx', 'const App = () => <p>Ready.</p>; console.info("Pi started.");'],
    ['src/App.jsx', 'const pi = 3.14; const App = () => <p>{pi}</p>;'],
    ['src/status.mjs', 'const pi = 3.14; for (let i = 0; i < 2; i++) { console.log(pi); } const read = () => pi;'],
    ['src/App.jsx', 'const App = () => <p>{count < 2 ? "Ready." : "Wait."}</p>; // Pi starts here.'],
    ['src/App.jsx', 'const App = () => <p title={count < 2 ? "Ready" : "Wait"} />; const pi = 3.14;'],
    ['src/App.jsx', 'const App = () => <p title={count < 2 ? "Ready" : "Wait"}></p>; console.info("Pi started.");'],
    ['src/App.jsx', 'const App = () => <p onClick={() => { if (count < 2) console.info("Pi started.") }}>Ready.</p>;'],
    ['src/App.jsx', 'const App = () => <p title={count < 2 ? "Ready" : "Wait"}>{pi}</p>;'],
    ['src/App.jsx', 'const App = () => <p title={count < 2 ? "Ready" : "Wait"}>'],
    ['src/App.jsx', 'const App = () => <p title={count < 2 ? "Ready" : "Wait"'],
  ])('keeps script comments and logs exempt in %s', (file, source) => {
    expect(forbiddenHarnessCopy(source, file)).toEqual([])
  })

  it('reports the line after a rendered URL', () => {
    const file = 'src/App.svelte'
    expect(forbiddenHarnessCopy('<p>See https://example.com\nfor Pi settings.</p>', file)).toEqual([
      { file, line: 2, word: 'Pi' },
    ])
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

  it.each([
    ['status.rs', 'eprintln!("event=pi_start Pi started.");', 'return Err("Pi failed.".into());'],
    ['App.svelte', '<script>console.info("Pi started.")</script>', '<p>See https://example.com for Pi settings.</p>'],
    ['App.jsx', 'console.info("Pi started.");', 'const App = () => <p>Pi answers here.</p>'],
    ['App.jsx', 'console.info("Pi started.");', 'const App = () => <p title={count < 2 ? "Ready" : "Wait"}>Pi answers here.</p>;'],
    ['App.jsx', 'console.info("Pi started.");', 'const App = () => <p title={count < 2 ? "Ready" : "Wait"}>See https://example.com for Pi settings.</p>;'],
  ])('checks every CLI root and returns failure for %s UI copy', (name, log, copy) => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-copy-lint-'))
    const frontend = path.join(root, 'src')
    const backend = path.join(root, 'src-tauri')
    fs.mkdirSync(frontend)
    fs.mkdirSync(backend)
    const fixture = path.join(name.endsWith('.rs') ? backend : frontend, name)
    const run = () => spawnSync(process.execPath, ['test/ui-copy-lint.mjs', frontend, backend], { encoding: 'utf8' })
    try {
      fs.writeFileSync(fixture, log)
      expect(run().status).toBe(0)
      fs.appendFileSync(fixture, `\n${copy}`)
      const failure = run()
      expect(failure.status).toBe(1)
      expect(failure.stderr).toContain(`${name}:2: forbidden UI copy: Pi`)
    } finally {
      fs.rmSync(root, { recursive: true, force: true })
    }
  })

  it('does not reject a word that only contains the same letters', () => {
    expect(forbiddenUiCopy('<p>Mail is available.</p>')).toEqual([])
    expect(forbiddenUiCopy('https://x.ai/bot/example')).toEqual([])
    expect(forbiddenUiCopy('https://muniment.ai/')).toEqual([])
    expect(forbiddenUiCopy('AI from x.ai')).toHaveLength(1)
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
