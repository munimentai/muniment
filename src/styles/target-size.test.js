import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const app = fs.readFileSync(path.join(process.cwd(), 'src/App.svelte'), 'utf8')
const style = app.match(/<style>([\s\S]*)<\/style>/)?.[1] ?? ''
const selectors = [
  '.thread-delete',
  '.thread-delete-confirm button',
  '.run-error button',
]

const ruleBody = (source, selector) => [
  ...source.matchAll(/([^{}]+)\{([^{}]*)\}/g),
].find(([, selectors]) => selectors.split(',').some((item) => item.trim() === selector))?.[2] ?? ''

const pixelValue = (body, property) => Number(body.match(new RegExp(`(?:^|;)\\s*${property}\\s*:\\s*(\\d+(?:\\.\\d+)?)px`, 'i'))?.[1])

const undersized = (source) => selectors.filter((selector) => {
  const body = ruleBody(source, selector)
  const width = pixelValue(body, 'min-width')
  const height = pixelValue(body, 'min-height')
  return !Number.isFinite(width) || width < 24 || !Number.isFinite(height) || height < 24
})

describe('signed-in shell target sizes', () => {
  it('keeps compact shell controls at least 24 by 24 CSS pixels', () => {
    expect(undersized(style)).toEqual([])
  })

  it('rejects a compact shell control below the target-size floor', () => {
    const drift = style.replace(/(\.thread-delete\s*\{[^{}]*min-height:\s*)24px/, (_, prefix) => `${prefix}23px`)
    expect(undersized(drift)).toContain('.thread-delete')
  })

  it('rejects a compact shell control without a minimum width', () => {
    const drift = style.replace(/(\.run-error button\s*\{[^{}]*)min-width:\s*24px;/, '$1')
    expect(undersized(drift)).toContain('.run-error button')
  })
})
