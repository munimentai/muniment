// @vitest-environment node
import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'
import { compile } from 'svelte/compiler'

// The build compiles each component with the Svelte compiler, which drops a
// scoped rule when no element in that component's markup can match it. A
// rule for an attribute or class that only a script sets matches nothing at
// compile time, so it never ships. Wrap the script-set part in :global(...)
// and keep the element part scoped, or delete a rule nothing uses.

// Repo root: `npm test` runs vitest with `--root .`.
const root = process.cwd()
const components = (dir = 'src') => fs.readdirSync(path.join(root, dir), { withFileTypes: true })
  .flatMap((entry) => entry.isDirectory() ? components(`${dir}/${entry.name}`)
    : entry.name.endsWith('.svelte') ? [`${dir}/${entry.name}`] : [])

const unusedSelectors = (file, source = fs.readFileSync(path.join(root, file), 'utf8')) =>
  compile(source, { filename: file, css: 'external' }).warnings
    .filter((warning) => warning.code === 'css_unused_selector')
    .map((warning) => `${file}:${warning.start?.line}:${warning.start?.column} ${warning.message}`)

describe('compiled component CSS', () => {
  it.each(components())('%s ships every rule it writes', (file) => {
    expect(unusedSelectors(file)).toEqual([])
  })

  it('reports a rule for an attribute only a script sets', () => {
    const source = '<div class="row"></div>\n<style>.row[data-open="true"] { color: red; }</style>'
    expect(unusedSelectors('Row.svelte', source)).toHaveLength(1)
    expect(unusedSelectors('Row.svelte', source.replace('.row[data-open="true"]', '.row:global([data-open="true"])'))).toEqual([])
  })
})
