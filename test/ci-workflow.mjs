// Runs the CI workflow's own Classify changes script and evaluates the job and
// step conditions against its outputs, so routing tests read the scheduled
// commands from the workflow instead of restating its rules.
import { readFileSync, existsSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import path from 'node:path'

export const mirrors = ['.github/workflows/ci.yml', '.forgejo/workflows/ci.yml']
const servers = { '.github/workflows/ci.yml': 'https://github.com', '.forgejo/workflows/ci.yml': 'http://forgejo.test' }

const indent = (line) => line.match(/^ */)[0].length
const unquote = (value) => value.trim().replace(/^(['"])([\s\S]*)\1$/, '$2')

// Reads a block scalar or a plain value of `key` at `depth` spaces in `lines`.
function field(lines, key, depth) {
  const index = lines.findIndex((line) => line.startsWith(`${' '.repeat(depth)}${key}:`))
  if (index < 0) return undefined
  const value = lines[index].slice(depth + key.length + 1).trim()
  if (!/^[|>][-+]?$/.test(value)) return unquote(value)
  const body = []
  for (const line of lines.slice(index + 1)) {
    if (line.trim() && indent(line) <= depth) break
    body.push(line)
  }
  while (body.length && !body.at(-1).trim()) body.pop()
  const margin = Math.min(...body.filter((line) => line.trim()).map(indent))
  return body.map((line) => line.slice(margin)).join('\n')
}

export function parseWorkflow(file) {
  const text = readFileSync(file, 'utf8')
  const lines = text.slice(text.indexOf('\njobs:\n') + 7).split('\n')
  const defaultsDir = field(text.split('\n'), 'defaults', 0) !== undefined
    ? text.match(/^defaults:\n  run:\n    working-directory: (.+)$/m)?.[1] : undefined
  const jobs = []
  for (const [index, line] of lines.entries()) {
    const name = line.match(/^  ([\w-]+):$/)?.[1]
    if (!name) continue
    const end = lines.findIndex((next, at) => at > index && /^ {0,2}\S/.test(next))
    const body = lines.slice(index + 1, end < 0 ? undefined : end)
    const stepsAt = body.findIndex((entry) => entry === '    steps:')
    const head = stepsAt < 0 ? body : body.slice(0, stepsAt)
    const matrix = {}
    const matrixAt = head.findIndex((entry) => entry === '      matrix:')
    if (matrixAt >= 0) {
      for (const entry of head.slice(matrixAt + 1)) {
        if (indent(entry) <= 6) break
        const [, key, values] = entry.match(/^ {8}([\w-]+): \[(.*)\]$/) ?? []
        if (key) matrix[key] = values.split(',').map(unquote)
      }
    }
    const steps = []
    for (const entry of stepsAt < 0 ? [] : body.slice(stepsAt + 1)) {
      if (entry.startsWith('      - ')) steps.push([`        ${entry.slice(8)}`])
      else if (steps.length) steps.at(-1).push(entry)
    }
    jobs.push({
      name,
      if: field(head, 'if', 4),
      workingDirectory: head.join('\n').match(/^ {4}defaults:\n {6}run:\n {8}working-directory: (.+)$/m)?.[1] ?? defaultsDir,
      matrix,
      steps: steps.map((step) => ({
        name: field(step, 'name', 8), id: field(step, 'id', 8), if: field(step, 'if', 8),
        run: field(step, 'run', 8), workingDirectory: field(step, 'working-directory', 8),
      })),
    })
  }
  return jobs
}

// A recursive-descent evaluator for the GitHub expression subset the
// workflow conditions use.
export function evaluate(expression, context) {
  if (expression === undefined) return true
  let source = expression.trim()
  const wrapped = source.match(/^\$\{\{([\s\S]*)\}\}$/)
  if (wrapped) source = wrapped[1]
  const tokens = [...source.matchAll(/\s+|'((?:[^']|'')*)'|(&&|\|\||==|!=|!|\(|\)|,)|([A-Za-z_][\w-]*(?:\.[A-Za-z_][\w-]*)*)|(\d+)|(.)/g)]
    .filter((match) => match[0].trim())
    .map(([text, string, operator, name, number, other]) => {
      if (other !== undefined) throw new Error(`Unsupported expression token ${other} in ${expression}`)
      if (string !== undefined) return { value: string.replaceAll("''", "'") }
      if (number !== undefined) return { value: Number(number) }
      if (name === 'true' || name === 'false') return { value: name === 'true' }
      if (name === 'null') return { value: null }
      return operator ? { op: operator } : { name: text }
    })
  let at = 0
  const peek = (op) => tokens[at]?.op === op
  const take = (op) => {
    if (!peek(op)) throw new Error(`Expected ${op} in ${expression}`)
    at += 1
  }
  const text = (value) => (value === null || value === undefined ? '' : String(value)).toLowerCase()
  const truthy = (value) => !(value === null || value === undefined || value === false || value === '' || value === 0)
  const functions = {
    success: () => true, always: () => true, cancelled: () => false, failure: () => false,
    contains: (haystack, needle) => Array.isArray(haystack)
      ? haystack.some((item) => text(item) === text(needle)) : text(haystack).includes(text(needle)),
    startswith: (value, prefix) => text(value).startsWith(text(prefix)),
    endswith: (value, suffix) => text(value).endsWith(text(suffix)),
  }
  const primary = () => {
    const token = tokens[at]
    if (!token) throw new Error(`Unexpected end of ${expression}`)
    if (token.op === '(') {
      at += 1
      const value = or()
      take(')')
      return value
    }
    at += 1
    if ('value' in token) return token.value
    if (!token.name) throw new Error(`Unexpected ${token.op} in ${expression}`)
    if (peek('(')) {
      at += 1
      const args = []
      while (!peek(')')) {
        args.push(or())
        if (peek(',')) at += 1
      }
      take(')')
      const fn = functions[token.name.toLowerCase()]
      if (!fn) throw new Error(`Unsupported function ${token.name} in ${expression}`)
      return fn(...args)
    }
    return token.name.split('.').reduce((value, key) => value?.[key], context) ?? null
  }
  const unary = () => {
    if (peek('!')) {
      at += 1
      return !truthy(unary())
    }
    return primary()
  }
  const compare = () => {
    let value = unary()
    while (peek('==') || peek('!=')) {
      const op = tokens[at].op
      at += 1
      const equal = text(value) === text(unary())
      value = op === '==' ? equal : !equal
    }
    return value
  }
  const and = () => {
    let value = compare()
    while (peek('&&')) {
      at += 1
      const right = compare()
      value = truthy(value) ? right : value
    }
    return value
  }
  const or = () => {
    let value = and()
    while (peek('||')) {
      at += 1
      const right = and()
      value = truthy(value) ? value : right
    }
    return value
  }
  const result = or()
  if (at !== tokens.length) throw new Error(`Trailing tokens in ${expression}`)
  return truthy(result)
}

// Runs the workflow's Classify changes script with a stub git that lists
// `paths` as the diff and knows `base` only when `knownBase` is true.
export function classify(file, paths, { event = 'pull_request', base = 'base', knownBase = true } = {}) {
  const changes = parseWorkflow(file).find((job) => job.name === 'changes')
  const script = changes.steps.find((step) => step.name === 'Classify changes').run
  const output = execFileSync('bash', ['-eu', '-c', `
    git() {
      if [ "$1" = cat-file ]; then [ "$KNOWN_BASE" = true ]; return; fi
      printf '%s' "$CHANGED_PATHS"
    }
    GITHUB_OUTPUT=$(mktemp)
    trap 'rm -f "$GITHUB_OUTPUT"' EXIT
    ${script}
    cat "$GITHUB_OUTPUT"
  `], {
    encoding: 'utf8',
    timeout: 10000,
    env: {
      ...process.env,
      GITHUB_EVENT_NAME: event,
      BASE_SHA: base,
      HEAD_SHA: 'head',
      KNOWN_BASE: String(knownBase),
      CHANGED_PATHS: paths.length ? `${paths.join('\n')}\n` : '',
    },
  })
  return Object.fromEntries(output.trim().split('\n').map((line) => line.split('=')))
}

const combinations = (matrix) => Object.entries(matrix).reduce((all, [key, values]) =>
  all.flatMap((combination) => values.map((value) => ({ ...combination, [key]: value }))), [{}])

// Lists every step that would run for `paths`, with its job, matrix entry,
// command and working directory. The classify step is skipped when the
// main-branch proof reuses a tested pull request tree.
export function route(file, paths, { event = 'pull_request', base = 'base', knownBase = true, reuse = 'false' } = {}) {
  const outputs = reuse === 'true' ? { reuse } : { reuse, ...classify(file, paths, { event, base, knownBase }) }
  const repository = 'owner/repo'
  const github = {
    event_name: event, server_url: servers[file], repository,
    event: event === 'pull_request' ? { pull_request: { head: { repo: { full_name: repository } } } } : {},
  }
  const needs = new Proxy({ changes: { result: 'success', outputs } }, {
    get: (target, key) => target[key] ?? { result: 'success', outputs: {} },
  })
  const scheduled = []
  for (const job of parseWorkflow(file)) {
    if (job.name === 'changes') continue
    for (const matrix of combinations(job.matrix)) {
      const context = { github, needs, matrix, vars: {}, inputs: {}, steps: {}, env: {} }
      if (!evaluate(job.if, context)) continue
      for (const step of job.steps) {
        if (!evaluate(step.if, context)) continue
        scheduled.push({ job: job.name, matrix, name: step.name, run: step.run ?? '',
          cwd: step.workingDirectory ?? job.workingDirectory ?? '.' })
      }
    }
  }
  return { outputs, scheduled }
}

// The Cargo.lock a cargo command resolves, walking up from its manifest or
// working directory as cargo does.
function lockfileFor(command, cwd) {
  const manifest = command.match(/--manifest-path[ =](\S+)/)?.[1]
  let directory = path.posix.normalize(manifest ? path.posix.join(cwd, path.posix.dirname(manifest)) : cwd)
  for (;;) {
    const lock = path.posix.join(directory, 'Cargo.lock')
    if (existsSync(lock)) return lock
    if (directory === '.' || directory === '/') return undefined
    directory = path.posix.dirname(directory)
  }
}

// The `cargo test` commands among scheduled steps, each with the lockfile it
// reads and whether it refuses to change it.
export function cargoTests(scheduled) {
  return scheduled.flatMap((step) => {
    let cwd = step.cwd
    const found = []
    for (const command of step.run.replace(/\\\n/g, ' ').split(/&&|\|\||;|\n/)) {
      const cd = command.match(/^\s*cd\s+(\S+)\s*$/)?.[1]
      if (cd) cwd = path.posix.normalize(path.posix.join(cwd, unquote(cd)))
      else if (/\bcargo\s+(?:\+\S+\s+)?(?:test|nextest\s+run)\b/.test(command)) {
        found.push({ step, command: command.trim(), lockfile: lockfileFor(command, cwd),
          locked: /--(?:locked|frozen)\b/.test(command) })
      }
    }
    return found
  })
}

export const coreWorkspaceTests = (scheduled) => cargoTests(scheduled)
  .filter(({ lockfile, locked }) => locked && lockfile === 'core/Cargo.lock')
export const boundaryChecks = (scheduled) => scheduled
  .filter((step) => /(?:^|[\s;&/])(?:\.\/)?scripts\/check-core-boundary\.sh\b/.test(step.run))
export const desktopPlatforms = (scheduled) => [...new Set(scheduled
  .filter((step) => step.job === 'desktop-compile' && /sudo desktop-ci/.test(step.run))
  .map((step) => step.matrix.platform))].sort()
