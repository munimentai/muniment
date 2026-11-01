import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

// Classes may be exempted only when markup intentionally exposes a class for a
// non-CSS consumer. Each entry must explain that consumer; the stale-entry test
// below keeps this from becoming a graveyard for removed markup.
const EXCEPTIONS = {}

// Repo root: `npm test` runs vitest with `--root .`.
const root = process.cwd()
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8')
const withoutComments = (source) => source.replace(/\/\*[\s\S]*?\*\//g, '')
const styleBlocks = (source) => [...source.matchAll(/<style(?:\s[^>]*)?>([\s\S]*?)<\/style>/g)]
  .map((match) => match[1])
  .join('\n')
const markup = (source) => source
  .replace(/<script(?:\s[^>]*)?>[\s\S]*?<\/script>/g, '')
  .replace(/<style(?:\s[^>]*)?>[\s\S]*?<\/style>/g, '')
  .replace(/<!--[\s\S]*?-->/g, '')

const classTokens = (source) => {
  const sourceMarkup = markup(source)
  const attributes = [...sourceMarkup.matchAll(/\bclass\s*=\s*(["'])(.*?)\1/gs)]
  const dynamic = [...sourceMarkup.matchAll(/\bclass\s*=\s*(\S)/g)]
    .filter(([, first]) => first !== '"' && first !== "'")
  if (dynamic.length || attributes.some(([, , value]) => /[{}]/.test(value))) {
    throw new Error('Unresolvable dynamic class expression; use literal class tokens or class:name directives')
  }

  return new Set([
    ...attributes.flatMap(([, , value]) => value.split(/\s+/).filter(Boolean)),
    ...[...sourceMarkup.matchAll(/\bclass:([A-Za-z_][\w-]*)/g)]
      .map(([, name]) => name),
  ])
}

const definedClasses = (source) => new Set(
  [...withoutComments(styleBlocks(source) || source).matchAll(/([^{}]+)\{[^{}]*\}/g)]
    .flatMap(([, selectors]) => [...selectors.matchAll(/\.([A-Za-z_][\w-]*)/g)])
    .map(([, name]) => name),
)

const globalClasses = new Set([
  ...definedClasses(read('src/styles/base.css')),
  ...definedClasses(read('src/styles/tokens.css')),
])

const missingClasses = (source, exceptions = {}) => {
  const defined = new Set([...globalClasses, ...definedClasses(source)])
  return [...classTokens(source)].filter((name) => !defined.has(name) && !(name in exceptions))
}

const components = (dir = 'src') => fs.readdirSync(path.join(root, dir), { withFileTypes: true })
  .flatMap((entry) => entry.isDirectory() ? components(`${dir}/${entry.name}`)
    : entry.name.endsWith('.svelte') ? [`${dir}/${entry.name}`] : [])
const COMPONENTS = components()

describe('markup class usage', () => {
  it.each(COMPONENTS)('%s defines every class used in markup', (file) => {
    expect(missingClasses(read(file), EXCEPTIONS[file] ?? {})).toEqual([])
  })

  it('allowlists only components and class tokens that still exist', () => {
    expect(Object.keys(EXCEPTIONS).filter((file) => !COMPONENTS.includes(file))).toEqual([])
    for (const [file, exceptions] of Object.entries(EXCEPTIONS)) {
      const used = classTokens(read(file))
      const stillUndefined = new Set(missingClasses(read(file)))
      expect(Object.keys(exceptions).filter((name) => !used.has(name) || !stillUndefined.has(name))).toEqual([])
    }
  })

  it('reports a class directive with no matching rule', () => {
    const shippedShape = '<main class:signed-frame={signedIn}></main><style>.other { color: inherit; }</style>'
    expect(missingClasses(shippedShape)).toEqual(['signed-frame'])
  })

  it('fails loudly on an unresolvable dynamic class expression', () => {
    expect(() => classTokens('<main class={frameClass}></main>'))
      .toThrow(/Unresolvable dynamic class expression/)
    expect(() => classTokens('<main class="frame {frameClass}"></main>'))
      .toThrow(/Unresolvable dynamic class expression/)
    expect(classTokens('<main class = "frame"></main>')).toEqual(new Set(['frame']))
  })
})
