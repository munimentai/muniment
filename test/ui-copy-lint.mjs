#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const FORBIDDEN = /\b(?:ai|magic|supercharg(?:e|es|ed|ing)|unlock(?:s|ed|ing)?|sovereignty)\b/giu
const EM_DASH = /\u{2014}|\\u(?:2014|\{0*2014\})|&(?:mdash|#0*8212|#x0*2014);/giu
const TEXT_SOURCE = /\.(?:css|html|js|json|jsx|md|mjs|rs|svelte|svg|toml|ts|tsx|txt|wxs|xml|yaml|yml)$/
const EXCLUDED_DIRECTORIES = new Set(['node_modules', 'target', 'third-party', '_ds'])

export function forbiddenUiCopy(source, file = '<fixture>') {
  return [...source.matchAll(FORBIDDEN)].map((match) => ({
    file,
    line: source.slice(0, match.index).split('\n').length,
    word: match[0],
  }))
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
  return sourceFiles(directory, /\.(?:js|svelte)$/)
    .filter((file) => !file.endsWith('.test.js'))
    .flatMap((file) => forbiddenUiCopy(fs.readFileSync(file, 'utf8'), file))
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
  const failures = [...lintUiCopy(roots[0]), ...lintEmDashes(roots)]
  for (const failure of failures) {
    console.error(`${path.relative(process.cwd(), failure.file)}:${failure.line}: forbidden UI copy: ${failure.word}`)
  }
  if (failures.length) process.exitCode = 1
}
