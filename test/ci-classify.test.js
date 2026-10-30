import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'

const root = process.cwd()
const fixtures = path.join(root, 'test/fixtures/ci-classify')
const script = path.join(root, 'scripts/ci/classify.sh')
const keys = ['release_tooling', 'release_only', 'docs_only', 'companion', 'attach_fixtures',
  'code_diff_fixtures', 'desktop', 'core_shared', 'core_support', 'installer']

// A fixture names changed paths, one per line. A file whose name starts with
// event- names the event, the base and whether git knows that base first.
const readFixture = (name) => {
  const lines = fs.readFileSync(path.join(fixtures, `${name}.paths`), 'utf8').split('\n').filter((line) => line !== '')
  if (!name.startsWith('event-')) return { event: 'pull_request', base: 'base', known: 'yes', paths: lines }
  return {
    event: lines[0].slice('event '.length),
    base: lines[1].slice('base '.length),
    known: lines[2].slice('known '.length),
    paths: lines.slice(3),
  }
}

const classify = ({ event, base, known, paths }) => {
  const run = spawnSync(script, [base, 'HEAD'], {
    encoding: 'utf8',
    timeout: 10000,
    env: {
      ...process.env,
      GITHUB_EVENT_NAME: event,
      KNOWN_BASE: known,
      CHANGED_PATHS: paths.length ? `${paths.join('\n')}\n` : '',
      PATH: `${path.join(fixtures, 'git-stub')}${path.delimiter}${process.env.PATH}`,
    },
  })
  expect(run.status, run.stderr).toBe(0)
  return run.stdout
}

describe('CI path classifier', () => {
  const names = fs.readdirSync(fixtures).filter((name) => name.endsWith('.paths')).map((name) => name.slice(0, -'.paths'.length))

  it('is executable and prints the ten keys in order', () => {
    expect(fs.statSync(script).mode & 0o111).not.toBe(0)
    const output = classify(readFixture('empty'))
    expect(output.trim().split('\n').map((line) => line.split('=')[0])).toEqual(keys)
  })

  it.each(names)('matches the recorded output for %s', (name) => {
    const expected = fs.readFileSync(path.join(fixtures, `${name}.out`), 'utf8')
    expect(classify(readFixture(name))).toBe(expected)
  })
})
