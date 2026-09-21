#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const FORBIDDEN = /\b(?:ai|magic|supercharg(?:e|es|ed|ing)|unlock(?:s|ed|ing)?|sovereignty)\b/giu
const HARNESS = /\bpi\b/giu
const TOKENS = /\/\/[^\n]*|\/\*[\s\S]*?\*\/|<!--[\s\S]*?-->|r(#+)"[\s\S]*?"\1|"(?:\\[\s\S]|[^"\\])*"|'(?:\\[^\n]|[^'\\\n])*'|`(?:\\[\s\S]|[^`\\])*`/g
const LOG_CALL = /\b(?:console\.(?:debug|info|log|warn|error)|(?:e?println|debug|info|warn|error|trace)!)\s*\(/g
const blank = (text) => text.replace(/[^\n]/g, ' ')

function markupTag(source, index, scriptToken) {
  const start = /<\/?(?:([a-zA-Z][\w:.-]*)(?=[\s/>])|(?=>))/y
  start.lastIndex = index
  const element = start.exec(source)
  if (!element) return null
  let depth = 0
  let end = start.lastIndex
  while (end < source.length) {
    // Strings and comments cannot close an attribute expression or the tag.
    if (depth || /["']/.test(source[end])) {
      scriptToken.lastIndex = end
      const token = scriptToken.exec(source)
      if (token) {
        end += token[0].length
        continue
      }
    }
    const char = source[end++]
    if (char === '{') depth += 1
    if (char === '}') {
      if (!depth) return null
      depth -= 1
    }
    if (!depth && char === '<') return null
    if (!depth && char === '>') return [source.slice(index, end), element[1]]
  }
  return null
}

// Keep rendered text out of the script tokenizer. URLs and slashes are text there.
function copyTokens(source, file) {
  if (!/\.(?:svelte|js|mjs|jsx|tsx)$/.test(file) && file !== '<fixture>') return [...source.matchAll(TOKENS)]
  const scriptToken = new RegExp(TOKENS.source, 'y')
  const contexts = []
  const tokens = []
  let mode = file.endsWith('.svelte') ? 'text' : 'code'
  let index = 0
  while (index < source.length) {
    if (source.startsWith('<!--', index)) {
      const end = source.indexOf('-->', index + 4)
      const value = source.slice(index, end < 0 ? source.length : end + 3)
      tokens.push(Object.assign([value], { index }))
      index += value.length
      continue
    }
    const element = markupTag(source, index, scriptToken)
    if (element) {
      for (const token of element[0].matchAll(TOKENS)) {
        token.index += index
        tokens.push(token)
      }
      if (element[0].startsWith('</')) {
        if (contexts.at(-1)?.kind === 'element') mode = contexts.pop().mode
      } else if (!element[0].endsWith('/>') && !/^(?:area|base|br|col|embed|hr|img|input|link|meta|param|source|track|wbr)$/.test(element[1])) {
        const raw = /^(?:script|style)$/.test(element[1])
        contexts.push({ kind: 'element', mode, rawEnd: raw ? source.indexOf(`</${element[1]}`, index + element[0].length) : -1 })
        mode = raw ? 'code' : 'text'
      }
      index += element[0].length
      continue
    }
    if (mode === 'text') {
      if (source[index] === '{') {
        contexts.push({ kind: 'expression', depth: 1 })
        mode = 'code'
        index += 1
      } else {
        const start = index++
        while (index < source.length && !/[<{]/.test(source[index])) index += 1
        tokens.push(Object.assign([source.slice(start, index)], { index: start, rendered: true }))
      }
      continue
    }
    scriptToken.lastIndex = index
    const token = scriptToken.exec(source)
    if (token) {
      // A raw element ends at its closing tag, even inside a script comment.
      const rawEnd = contexts.at(-1)?.rawEnd
      if (rawEnd > index && rawEnd < index + token[0].length) token[0] = source.slice(index, rawEnd)
      tokens.push(token)
      index += token[0].length
      continue
    }
    const expression = contexts.at(-1)
    if (expression?.kind === 'expression') {
      if (source[index] === '{') expression.depth += 1
      if (source[index] === '}' && --expression.depth === 0) {
        contexts.pop()
        mode = 'text'
      }
    }
    index += 1
  }
  return tokens
}

function withoutLogsAndComments(source, tokens) {
  let masked = source
  for (const token of tokens) {
    if (!token.rendered && /^(?:\/\/|\/\*|<!--)/.test(token[0])) {
      masked = masked.slice(0, token.index) + blank(token[0]) + masked.slice(token.index + token[0].length)
    }
  }
  // Balance calls so a log never exempts another string on the same line.
  for (const call of [...masked.matchAll(LOG_CALL)]) {
    if (tokens.some((token) => call.index >= token.index && call.index < token.index + token[0].length)) continue
    let depth = 1
    let end = call.index + call[0].length
    while (end < source.length && depth) {
      const token = tokens.find((token) => token.index === end)
      if (token) { end += token[0].length; continue }
      if (source[end] === '(') depth += 1
      if (source[end] === ')') depth -= 1
      end += 1
    }
    masked = masked.slice(0, call.index) + blank(source.slice(call.index, end)) + masked.slice(end)
  }
  return masked
}

// These exact diagnostics stay inside the runtime. Public boundaries translate them into UI copy.
const DIAGNOSTICS = new Map([
  ['active_run.rs', ['Pi queued message must not be empty']],
  ['chat_coordinate.rs', ['new runs prepare a Pi prompt', 'timed out waiting for Pi stream']],
  ['chat_profile.rs', ['could not create the Pi session root']],
  ['journal/reducer.rs', ['run cannot acquire Pi', 'Pi acquisition has not started', 'Pi session may only be bound once', 'Pi session binding payload is invalid']],
  ['pi_packages.rs', [
    'Pi package acquisition failed.', 'Pi package acquisition timed out.', 'Pi package acquisition did not install the pinned versions.',
    // Exact upstream copy matched when branding the pinned MCP adapter.
    'You can close this page and return to Pi.',
    String.raw`export function getAppName(): string {\n  const name = readPiConfig()?.name\n  return typeof name === \"string\" && name.trim() ? name.trim() : \"pi\"\n}`,
    // Exact source literals used to migrate the pinned extension's private paths.
    "'.pi'", "parts[0] === '.pi'", "['.pi', '.muniment'].includes(parts[0])",
    "join(ctx.cwd, '.pi', 'tasks', runId); join('.pi', 'tasks', runId);", 'Output is written to .pi/tasks',
  ]],
  ['pi_settings.rs', ['The Pi directory URL is invalid.', 'Cannot locate the Pi home directory.', 'Pi settings lock changed owners.']],
  ['sidecar/io.rs', ['timed out writing Pi RPC stdin', 'Pi stderr {index}', 'Pi stderr 5']],
  ['sidecar/pi.rs', [
    'Pi session directory is unavailable', 'Pi session locator is invalid', 'Pi session file is unavailable', 'Pi session state is invalid',
    'invalid Pi RPC JSON: {error}', 'spawn Pi RPC dispatcher', 'timed out waiting for Pi RPC call lock',
    'Pi RPC command must be a JSON object', 'Pi RPC command must contain a string `type`',
    'Pi RPC transport belongs to a replaced child generation', 'unexpected Pi readiness response: {response}',
    'timed out waiting for Pi RPC response `{id}`', 'Pi RPC dispatcher stopped while waiting for `{id}`',
  ]],
  ['sidecar/pi_chat.rs', [
    'Pi frame is missing type', 'Pi queued message must not be empty', 'Pi queue command failed', 'Pi queue command is JSON serializable',
    'Pi did not acknowledge the prompt within 30 seconds. {error}', 'Pi rejected the prompt', 'Pi session binding failed',
    'Pi cancellation did not finish', 'timed out waiting for Pi stream', 'Pi process stream ended',
  ]],
  ['sidecar/pi_install.rs', ['Pi installation failed: {:?}']],
].map(([file, messages]) => [`/core/src/${file}`, new Set(messages)]))

export function forbiddenHarnessCopy(source, file = '<fixture>') {
  const normalizedFile = `/${file.replaceAll('\\', '/')}`
  // The scan registry names installed assistants as data, not shell prose.
  if (normalizedFile.endsWith('/core/src/harness_scan.rs')) return []
  const tokens = copyTokens(source, file)
  const copy = withoutLogsAndComments(source, tokens)
  const strings = tokens.filter((token) => token.rendered || !/^(?:\/\/|\/\*|<!--)/.test(token[0]))
  const diagnostics = [...DIAGNOSTICS].find(([suffix]) => normalizedFile.endsWith(suffix))?.[1]
  return [...copy.matchAll(HARNESS)].filter((match) => {
    const token = strings.find((token) => match.index >= token.index && match.index < token.index + token[0].length)
    if (!token) return file === '<fixture>' || file.endsWith('.svelte')
    if (token.rendered) return true
    const value = token[0].slice(1, -1)
    if (/\bevent=[\w.-]+/.test(value)) return false
    if (diagnostics?.has(value)) return false
    // Paths and runtime protocol identifiers are not prose.
    if (value === 'acquiring-pi' || (/^[\w./~:@{}-]+$/.test(value) && value.includes('/'))) return false
    if (file.endsWith('.rs') && /^[a-z0-9_.:@{}-]+$/.test(value) && /[-.:@{}]/.test(value)) return false
    if (normalizedFile.includes('/core/src/') && value === 'pi') return false
    if (normalizedFile.endsWith('/core/src/memory_runtime.rs') && (
      /function \($/.test(copy.slice(token.index, match.index)) || copy.slice(match.index).startsWith('pi.registerTool')
    )) return false
    return true
  }).map((match) => ({
    file,
    line: copy.slice(0, match.index).split('\n').length,
    word: match[0],
  }))
}
const EM_DASH = /\u{2014}|\\u(?:2014|\{0*2014\})|&(?:mdash|#0*8212|#x0*2014);/giu
const TEXT_SOURCE = /\.(?:css|html|js|json|jsx|md|mjs|rs|svelte|svg|toml|ts|tsx|txt|wxs|xml|yaml|yml)$/
const EXCLUDED_DIRECTORIES = new Set(['node_modules', 'target', 'third-party', '_ds'])

export function forbiddenUiCopy(source, file = '<fixture>') {
  // Official product and provider hosts are domains, not promotional copy.
  const prose = source.replace(/\b(?:x|muniment|claude)\.ai\b/g, (host) => " ".repeat(host.length))
  return [...prose.matchAll(FORBIDDEN)].map((match) => ({
    file,
    line: source.slice(0, match.index).split('\n').length,
    word: match[0],
  })).concat(forbiddenHarnessCopy(source, file))
}

function sourceFiles(directory, include = TEXT_SOURCE) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const target = path.join(directory, entry.name)
    if (entry.isDirectory()) {
      return EXCLUDED_DIRECTORIES.has(entry.name) ? [] : sourceFiles(target, include)
    }
    if (!include.test(entry.name)) return []
    return [target]
  })
}

export function lintUiCopy(directory) {
  return sourceFiles(directory, /\.(?:js|jsx|mjs|ts|tsx|svelte|rs)$/)
    .filter((file) => !file.endsWith('.test.js') && !(file.endsWith('.rs') && /(?:^|[/\\])tests(?:[/\\]|\.rs$)/.test(file)))
    .flatMap((file) => {
      const source = fs.readFileSync(file, 'utf8')
      return /\.(?:js|svelte)$/.test(file)
        ? forbiddenUiCopy(source, file)
        : forbiddenHarnessCopy(source, file)
    })
}

export function forbiddenEmDashes(source, file = '<fixture>') {
  return [...source.matchAll(EM_DASH)].map((match) => ({
    file,
    line: source.slice(0, match.index).split('\n').length,
    word: match[0],
  }))
}

export function lintEmDashes(directories) {
  return directories.flatMap((directory) =>
    sourceFiles(directory).flatMap((file) => forbiddenEmDashes(fs.readFileSync(file, 'utf8'), file)),
  )
}

const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : ''
if (invokedPath === fileURLToPath(import.meta.url)) {
  const roots = (process.argv.slice(2).length ? process.argv.slice(2) : ['src']).map((root) => path.resolve(root))
  const failures = [...roots.flatMap(lintUiCopy), ...lintEmDashes(roots)]
  for (const failure of failures) {
    console.error(`${path.relative(process.cwd(), failure.file)}:${failure.line}: forbidden UI copy: ${failure.word}`)
  }
  if (failures.length) process.exitCode = 1
}
