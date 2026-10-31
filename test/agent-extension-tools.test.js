// @vitest-environment node
import { afterEach, beforeAll, describe, expect, it } from 'vitest'
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { loadAgentExtension } from './e2e/support/agent-extension.mjs'

let extension
beforeAll(async () => { extension = await loadAgentExtension() })
const cleanups = []
afterEach(() => { for (const cleanup of cleanups.splice(0)) cleanup() })

function tree(files) {
  const root = mkdtempSync(join(tmpdir(), 'muniment-search-'))
  cleanups.push(() => rmSync(root, { recursive: true, force: true }))
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(join(root, path, '..'), { recursive: true })
    writeFileSync(join(root, path), content)
  }
  return root
}

const unavailable = { find: 'fd is not available and could not be downloaded', grep: 'ripgrep (rg) is not available and could not be downloaded' }
// Runs the tool_result handler as Pi does after a native find or grep failed to start.
const search = (toolName, input, cwd, signal) => extension.handlers.tool_result(
  { type: 'tool_result', toolName, toolCallId: 'call', input, content: [{ type: 'text', text: unavailable[toolName] }], details: {}, isError: true },
  { cwd, signal },
)
const text = result => result.content.map(part => part.text).join('')

// Checks a value against the subset of JSON Schema the registered parameters use.
function schemaErrors(schema, value, at = 'root') {
  const errors = []
  if (schema.type === 'object') {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return [`${at} is not an object`]
    for (const key of schema.required || []) if (!(key in value)) errors.push(`${at}.${key} is missing`)
    for (const [key, item] of Object.entries(value)) {
      if (schema.properties?.[key]) errors.push(...schemaErrors(schema.properties[key], item, `${at}.${key}`))
      else if (schema.additionalProperties === false) errors.push(`${at}.${key} is not allowed`)
    }
  } else if (schema.type === 'array') {
    if (!Array.isArray(value)) return [`${at} is not an array`]
    if (schema.minItems !== undefined && value.length < schema.minItems) errors.push(`${at} has too few items`)
    if (schema.maxItems !== undefined && value.length > schema.maxItems) errors.push(`${at} has too many items`)
    value.forEach((item, index) => errors.push(...schemaErrors(schema.items, item, `${at}[${index}]`)))
  } else if (schema.type === 'string') {
    if (typeof value !== 'string') return [`${at} is not a string`]
    if (schema.minLength !== undefined && value.length < schema.minLength) errors.push(`${at} is too short`)
    if (schema.maxLength !== undefined && value.length > schema.maxLength) errors.push(`${at} is too long`)
  } else if (schema.type === 'boolean' && typeof value !== 'boolean') {
    errors.push(`${at} is not a boolean`)
  }
  return errors
}

describe('search fallback', () => {
  const files = {
    'a.txt': 'hello needle\nplain\n',
    'sub dir/b.txt': 'first\nNeedle two\nlast\n',
    'sub dir/c.md': 'a.b literal\naxb\n',
    'deep/x/y/z.txt': 'needle deep\n',
    '.hidden/h.txt': 'needle hidden\n',
    'node_modules/pkg/n.txt': 'needle module\n',
    'bin.dat': 'needle\u0000binary',
  }

  it('finds files by name and by relative path pattern, with spaces and hidden folders', async () => {
    const root = tree(files)
    const all = await search('find', { pattern: '*.txt' }, root)
    expect(all.isError).toBe(false)
    expect(text(all).split('\n').sort()).toEqual(['.hidden/h.txt', 'a.txt', 'deep/x/y/z.txt', 'sub dir/b.txt'])
    expect(text(await search('find', { pattern: 'sub dir/*.md' }, root))).toBe('sub dir/c.md')
    expect(text(await search('find', { pattern: '**/*.{md,dat}' }, root)).split('\n').sort()).toEqual(['bin.dat', 'sub dir/c.md'])
    expect(text(await search('find', { pattern: 'x' }, root))).toBe('deep/x/')
    expect(text(await search('find', { pattern: '?.txt', path: 'sub dir' }, root))).toBe('b.txt')
    expect(text(await search('find', { pattern: '*.txt', path: join(root, 'deep') }, '/'))).toBe('x/y/z.txt')
  })

  it('matches file contents with context, case and literal options and a glob filter', async () => {
    const root = tree(files)
    const plain = await search('grep', { pattern: 'needle' }, root)
    expect(plain.isError).toBe(false)
    expect(text(plain).split('\n').sort()).toEqual(['.hidden/h.txt:1: needle hidden', 'a.txt:1: hello needle', 'deep/x/y/z.txt:1: needle deep'])
    expect(text(await search('grep', { pattern: 'needle', ignoreCase: true, glob: 'b.txt', context: 1 }, root)))
      .toBe('sub dir/b.txt-1- first\nsub dir/b.txt:2: Needle two\nsub dir/b.txt-3- last')
    expect(text(await search('grep', { pattern: 'a.b', literal: true }, root))).toBe('sub dir/c.md:1: a.b literal')
    expect(text(await search('grep', { pattern: 'a.b', glob: '**/*.md' }, root))).toBe('sub dir/c.md:1: a.b literal\nsub dir/c.md:2: axb')
    expect(text(await search('grep', { pattern: 'two', path: 'sub dir/b.txt' }, root))).toBe('b.txt:2: Needle two')
  })

  it('reports no matches and missing paths as the native tools do', async () => {
    const root = tree(files)
    expect(await search('find', { pattern: '*.pdf' }, root)).toMatchObject({ isError: false, content: [{ type: 'text', text: 'No files found matching pattern' }] })
    expect(await search('grep', { pattern: 'absent' }, root)).toMatchObject({ isError: false, content: [{ type: 'text', text: 'No matches found' }] })
    expect(await search('grep', { pattern: '(' }, root)).toMatchObject({ isError: true })
    const missing = await search('find', { pattern: '*', path: 'nowhere' }, root)
    expect(missing.isError).toBe(true)
    expect(text(missing)).toContain('Path not found')
  })

  it('reads Windows separators in patterns and always reports forward slashes', async () => {
    expect(extension.module.searchGlob('sub dir\\*.txt', true).test('sub dir/b.txt')).toBe(true)
    expect(extension.module.searchGlob('**\\*.TXT', true).test('SUB DIR/B.txt')).toBe(true)
    expect(extension.module.searchGlob('sub dir/*.txt', false).test('sub dir/b.txt')).toBe(true)
    expect(extension.module.searchGlob('*.txt', false).test('sub dir/b.txt')).toBe(false)
    const root = tree(files)
    for (const result of [await search('find', { pattern: '*.txt' }, root), await search('grep', { pattern: 'needle' }, root)]) {
      expect(text(result)).not.toContain('\\')
    }
  })

  it('stops at the result limit and says so', async () => {
    const root = tree(Object.fromEntries(Array.from({ length: 5 }, (_, i) => [`f${i}.txt`, 'match\nmatch\n'])))
    const found = await search('find', { pattern: '*.txt', limit: 2 }, root)
    expect(text(found)).toBe('f0.txt\nf1.txt\n[2 results limit reached. Use limit=4 for more, or refine pattern]')
    expect(found.details).toEqual({ resultLimitReached: 2 })
    const grepped = await search('grep', { pattern: 'match', limit: 3 }, root)
    expect(text(grepped)).toBe('f0.txt:1: match\nf0.txt:2: match\nf1.txt:1: match\n[3 matches limit reached. Use limit=6 for more, or refine pattern]')
    expect(grepped.details).toEqual({ matchLimitReached: 3 })
  })

  it('stops a cancelled search with an error result', async () => {
    const root = tree(files)
    const controller = new AbortController()
    controller.abort()
    for (const tool of ['find', 'grep']) {
      expect(await search(tool, { pattern: 'needle' }, root, controller.signal)).toMatchObject({ isError: true, content: [{ type: 'text', text: 'Operation aborted' }] })
    }
  })

  it('lists a linked folder without following it out of the tree or around a cycle', async () => {
    const outside = tree({ 'secret.txt': 'needle outside\n' })
    const root = tree({ 'inside.txt': 'needle inside\n' })
    symlinkSync(outside, join(root, 'outside'), 'junction')
    symlinkSync(root, join(root, 'loop'), 'junction')
    const found = await search('find', { pattern: '*' }, root)
    expect(text(found).split('\n').sort()).toEqual(['inside.txt', 'loop', 'outside'])
    expect(text(await search('grep', { pattern: 'needle' }, root))).toBe('inside.txt:1: needle inside')
  })

  it('leaves native results and other failures untouched', async () => {
    const root = tree(files)
    const handler = extension.handlers.tool_result
    expect(await handler({ toolName: 'find', input: { pattern: '*' }, content: [{ type: 'text', text: 'a.txt' }], isError: false }, { cwd: root })).toBeUndefined()
    expect(await handler({ toolName: 'grep', input: { pattern: 'x' }, content: [{ type: 'text', text: 'Path not found: x' }], isError: true }, { cwd: root })).toBeUndefined()
    expect(await handler({ toolName: 'read', input: {}, content: [{ type: 'text', text: unavailable.find }], isError: true }, { cwd: root })).toBeUndefined()
  })
})

describe('ask_user_question arguments', () => {
  const tool = () => extension.tools.ask_user_question
  // Pi prepares the arguments, then validates them against the registered schema.
  const prepare = args => {
    const prepared = tool().prepareArguments(args)
    return { prepared, errors: schemaErrors(tool().parameters, prepared) }
  }

  it('documents a canonical example that the schema accepts', () => {
    const example = JSON.parse(tool().description.split('Arguments example: ')[1])
    expect(schemaErrors(tool().parameters, example)).toEqual([])
    expect(tool().prepareArguments(example)).toBe(example)
  })

  it('normalizes a question without the questions list and with choices', () => {
    const { prepared, errors } = prepare({ question: 'Which format?', choices: ['PDF', 'CSV'] })
    expect(errors).toEqual([])
    expect(prepared).toEqual({ questions: [{ id: 'question-1', question: 'Which format?', options: [{ label: 'PDF' }, { label: 'CSV' }] }] })
  })

  it('normalizes questions with choices and questions without ids to unique ids', () => {
    const { prepared, errors } = prepare({ questions: [
      { question: 'Which format?', choices: [{ label: 'PDF', description: 'Print' }, 'CSV'], multiSelect: true },
      { id: 'question-1', question: 'Which folder?' },
      'Anything else?',
    ] })
    expect(errors).toEqual([])
    expect(prepared.questions).toEqual([
      { id: 'question-2', question: 'Which format?', multiSelect: true, options: [{ label: 'PDF', description: 'Print' }, { label: 'CSV' }] },
      { id: 'question-1', question: 'Which folder?' },
      { id: 'question-3', question: 'Anything else?' },
    ])
    expect(tool().prepareArguments(prepared)).toBe(prepared)
  })

  it('rejects other shapes with the canonical example', () => {
    for (const args of [{}, { foo: 1 }, { questions: 'Which format?' }, { questions: [{ id: 'a', question: 'Q', header: 'H' }] },
      { questions: [{ question: 'Q', options: ['A'], choices: ['B'] }] }, { questions: [42] }]) {
      expect(() => tool().prepareArguments(args), JSON.stringify(args)).toThrow(/Example: \{"questions"/)
    }
  })

  it('keeps the schema bounds', () => {
    const question = i => ({ id: `q${i}`, question: 'Q' })
    expect(prepare({ questions: [1, 2, 3, 4, 5].map(question) }).errors).toContain('root.questions has too many items')
    expect(prepare({ questions: [] }).errors).toContain('root.questions has too few items')
    expect(prepare({ questions: [{ id: 'q', question: 'Q', choices: ['1', '2', '3', '4', '5', '6', '7'] }] }).errors).toContain('root.questions[0].options has too many items')
    expect(prepare({ questions: [{ id: 'q', question: '' }] }).errors).toContain('root.questions[0].question is too short')
  })

  const run = async (args, answer, signal) => {
    const calls = []
    const context = { ui: { editor: async (title, prefill) => {
      calls.push([title, JSON.parse(prefill)])
      return typeof answer === 'function' ? answer() : answer === undefined ? undefined : JSON.stringify(answer)
    } } }
    const result = await tool().execute('call', tool().prepareArguments(args), signal, undefined, context)
    return { calls, result }
  }

  it('sends the normalized payload and correlates the answers by id', async () => {
    const { calls, result } = await run({ question: 'Which format?', choices: ['PDF', 'CSV'] },
      { answers: [{ id: 'question-1', selected: ['PDF'], text: '' }] })
    expect(calls).toEqual([['muniment:ask_user_question', { questions: [{ id: 'question-1', question: 'Which format?', options: [{ label: 'PDF' }, { label: 'CSV' }] }] }]])
    expect(result.details).toEqual({ answers: [{ id: 'question-1', selected: ['PDF'], text: '' }] })
  })

  it('rejects duplicate ids and answers that do not match the questions', async () => {
    const args = { questions: [{ id: 'f', question: 'Which format?', options: [{ label: 'PDF' }, { label: 'CSV' }] }] }
    await expect(run({ questions: [{ id: 'a', question: 'One' }, { id: 'a', question: 'Two' }] }, { answers: [] })).rejects.toThrow('Question ids must be unique.')
    await expect(run(args, { answers: [] })).rejects.toThrow('The answers are incomplete.')
    await expect(run(args, { answers: [{ id: 'other', selected: ['PDF'], text: '' }] })).rejects.toThrow('The answer does not match the question.')
    await expect(run(args, { answers: [{ id: 'f', selected: ['Word'], text: '' }] })).rejects.toThrow('The answer does not match the question.')
    await expect(run(args, { answers: [{ id: 'f', selected: ['PDF', 'CSV'], text: '' }] })).rejects.toThrow('The answer does not match the question.')
    await expect(run(args, { answers: [{ id: 'f', selected: [], text: ' ' }] })).rejects.toThrow('The answer does not match the question.')
    const written = await run(args, { answers: [{ id: 'f', selected: [], text: 'Markdown' }] })
    expect(written.result.details.answers[0].text).toBe('Markdown')
  })

  it('returns a cancelled result without an answer', async () => {
    const args = { questions: [{ id: 'f', question: 'Which format?' }] }
    const unanswered = await run(args, undefined)
    expect(unanswered.result).toMatchObject({ details: { cancelled: true }, content: [{ type: 'text', text: 'The user did not answer. Do not infer an answer.' }] })
    const controller = new AbortController()
    const pending = run(args, () => new Promise(() => {}), controller.signal)
    controller.abort()
    expect((await pending).result.details).toEqual({ cancelled: true })
    const aborted = await run(args, { answers: [] }, controller.signal)
    expect(aborted.calls).toEqual([])
    expect(aborted.result.content[0].text).toBe('The question was cancelled.')
  })

  it('stays a question tool, not a permission approval', () => {
    expect(tool().description).toContain('This is not a permission or approval tool.')
  })
})
