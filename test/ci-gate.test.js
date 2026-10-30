import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'

const root = process.cwd()
const gate = path.join(root, 'scripts/ci/gate.sh')

// A checkout whose only commit beyond origin/main changes `file`, so the gate
// classifies a real diff. The script resolves its own checkout, so the temp
// repo only has to name the base.
const repo = (file) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-ci-gate-'))
  const run = (args, cwd = directory) => {
    const result = spawnSync('git', args, { cwd, encoding: 'utf8' })
    expect(result.status, result.stderr).toBe(0)
  }
  run(['init', '-q', '-b', 'main'])
  run(['config', 'user.email', 'gate@example.test'])
  run(['config', 'user.name', 'Gate'])
  fs.writeFileSync(path.join(directory, 'README.md'), 'base\n')
  run(['add', 'README.md'])
  run(['commit', '-q', '-m', 'base'])
  run(['branch', 'origin/main'])
  fs.mkdirSync(path.dirname(path.join(directory, file)), { recursive: true })
  fs.writeFileSync(path.join(directory, file), 'change\n')
  run(['add', '--', file])
  run(['commit', '-q', '-m', 'change'])
  return directory
}

const dryRun = (directory, gateName) => {
  const result = spawnSync(gate, [gateName], {
    cwd: directory,
    encoding: 'utf8',
    timeout: 20000,
    env: { ...process.env, GATE_DRY_RUN: '1', GIT_DIR: path.join(directory, '.git'),
      GIT_WORK_TREE: directory },
  })
  return result
}

const commands = (output, prefix) => output.split('\n').filter((line) => line.startsWith(prefix)).map((line) => line.slice(prefix.length))

describe('factory CI gate', () => {
  it('rejects an argument other than lint or unit_tests', () => {
    const result = spawnSync(gate, ['build'], { encoding: 'utf8' })
    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain('usage: scripts/ci/gate.sh <lint|unit_tests>')
  })

  it('runs the frontend suite and skips core, src-tauri and Go tests for one Svelte file', () => {
    const directory = repo('src/Widget.svelte')
    try {
      const result = dryRun(directory, 'unit_tests')
      expect(result.status, result.stderr).toBe(0)
      const skipped = commands(result.stdout, 'skip: ')
      const run = commands(result.stdout, 'run: ')
      expect(result.stdout.indexOf('gate: skipped')).toBeLessThan(result.stdout.indexOf('gate: run'))
      expect(run.some((command) => command.includes('npx vitest run'))).toBe(true)
      expect(run.some((command) => command.includes('node --test test/ci-routing.node.mjs'))).toBe(true)
      expect(run.some((command) => command.includes('python3 -B scripts/test_check_steering.py'))).toBe(true)
      expect(run.some((command) => command.includes('scripts/test_release.py'))).toBe(true)
      expect(skipped.some((command) => command.includes('cargo test --workspace'))).toBe(true)
      expect(skipped.some((command) => command.includes('cargo test --manifest-path src-tauri/Cargo.toml'))).toBe(true)
      expect(skipped.some((command) => command.includes('go test'))).toBe(true)
      expect(run.some((command) => command.includes('cargo test'))).toBe(false)
    } finally {
      fs.rmSync(directory, { recursive: true, force: true })
    }
  })

  it('runs the src-tauri checks for a desktop change under src-tauri', () => {
    const directory = repo('src-tauri/reader/main.go')
    try {
      const result = dryRun(directory, 'lint')
      expect(result.status, result.stderr).toBe(0)
      const run = commands(result.stdout, 'run: ')
      expect(run.some((command) => command.includes('test/cli-dependency-boundary.sh'))).toBe(true)
      expect(run.some((command) => command.includes('cargo fmt --manifest-path src-tauri/Cargo.toml'))).toBe(true)
      expect(run.some((command) => command.includes('gofmt'))).toBe(true)
    } finally {
      fs.rmSync(directory, { recursive: true, force: true })
    }
  })

  it('selects the core boundary check, both clippy passes and fmt for a core crate change', () => {
    const directory = repo('core/crates/example/src/lib.rs')
    try {
      const result = dryRun(directory, 'lint')
      expect(result.status, result.stderr).toBe(0)
      const run = commands(result.stdout, 'run: ')
      expect(run.some((command) => command.includes('scripts/check-core-boundary.sh'))).toBe(true)
      expect(run.filter((command) => command.includes('cd core &&') && command.includes('cargo clippy')).length).toBe(2)
      expect(run.some((command) => command.includes('cargo fmt --all --check'))).toBe(true)
    } finally {
      fs.rmSync(directory, { recursive: true, force: true })
    }
  })

  it('selects every lane and says so when origin/main is absent', () => {
    const directory = repo('src/Widget.svelte')
    spawnSync('git', ['update-ref', '-d', 'refs/heads/origin/main'], { cwd: directory })
    try {
      for (const gateName of ['lint', 'unit_tests']) {
        const result = dryRun(directory, gateName)
        expect(result.status, `${gateName}\n${result.stderr}`).toBe(0)
        expect(result.stdout.split('\n')[0]).toBe('gate: no origin/main ref; selecting every lane')
        expect(result.stdout).not.toContain('\nskip: ')
        expect(commands(result.stdout, 'run: ').length).toBeGreaterThan(0)
      }
    } finally {
      fs.rmSync(directory, { recursive: true, force: true })
    }
  })
})
