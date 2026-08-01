#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const FORBIDDEN = /\b(?:ai|magic|supercharg(?:e|es|ed|ing)|unlock(?:s|ed|ing)?|sovereignty)\b/giu

export function forbiddenUiCopy(source, file = '<fixture>') {
  return [...source.matchAll(FORBIDDEN)].map((match) => ({
    file,
    line: source.slice(0, match.index).split('\n').length,
    word: match[0],
  }))
}

function sourceFiles(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const target = path.join(directory, entry.name)
    if (entry.isDirectory()) return sourceFiles(target)
    if (!/\.(?:js|svelte)$/.test(entry.name) || entry.name.endsWith('.test.js')) return []
    return [target]
  })
}

export function lintUiCopy(directory) {
  return sourceFiles(directory).flatMap((file) => forbiddenUiCopy(fs.readFileSync(file, 'utf8'), file))
}

const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : ''
if (invokedPath === fileURLToPath(import.meta.url)) {
  const root = path.resolve(process.argv[2] ?? 'src')
  const failures = lintUiCopy(root)
  for (const failure of failures) {
    console.error(`${path.relative(process.cwd(), failure.file)}:${failure.line}: forbidden UI copy: ${failure.word}`)
  }
  if (failures.length) process.exitCode = 1
}
