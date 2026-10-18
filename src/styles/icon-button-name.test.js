import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

// Every button that shows only an icon carries its name in aria-label or
// aria-labelledby, so a screen reader never reads "button" alone. A button
// with words, a bound expression or visually hidden text names itself.
const root = process.cwd()
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8')
const components = (dir = 'src') => fs.readdirSync(path.join(root, dir), { withFileTypes: true })
  .flatMap((entry) => entry.isDirectory() ? components(`${dir}/${entry.name}`)
    : entry.name.endsWith('.svelte') ? [`${dir}/${entry.name}`] : [])
const COMPONENTS = components()

const markup = (source) => source
  .replace(/<script(?:\s[^>]*)?>[\s\S]*?<\/script>/g, '')
  .replace(/<style(?:\s[^>]*)?>[\s\S]*?<\/style>/g, '')
  .replace(/<!--[\s\S]*?-->/g, '')

// The end of a start tag: the first `>` outside quotes and Svelte braces.
function tagEnd(source, from) {
  let quote
  let braces = 0
  for (let index = from; index < source.length; index += 1) {
    const character = source[index]
    if (quote) {
      if (character === quote && source[index - 1] !== '\\') quote = undefined
    } else if (braces === 0 && (character === '"' || character === "'")) quote = character
    else if (character === '{') braces += 1
    else if (character === '}') braces -= 1
    else if (character === '>' && braces === 0) return index
  }
  return -1
}

export function buttons(source) {
  const found = []
  const text = markup(source)
  for (const start of text.matchAll(/<button\b/g)) {
    const end = tagEnd(text, start.index)
    if (end < 0) continue
    const tag = text.slice(start.index, end + 1)
    const close = text.indexOf('</button>', end)
    found.push({ tag, content: close < 0 ? '' : text.slice(end + 1, close) })
  }
  return found
}

// Removes every tag, reading each to its real end, so an arrow function in an
// attribute never leaves part of the tag behind as words.
function withoutTags(content) {
  let out = ''
  let index = 0
  while (index < content.length) {
    const open = content.indexOf('<', index)
    if (open < 0) { out += content.slice(index); break }
    out += content.slice(index, open)
    const end = tagEnd(content, open)
    if (end < 0) break
    index = end + 1
  }
  return out
}

// What a reader hears from the content: words and bound expressions. Tags,
// icons, snippets, raw HTML and block markers carry no name.
const spoken = (content) => withoutTags(content)
  .replace(/\{[#:/][^}]*\}/g, '')
  .replace(/\{@(?:html|render|const)[^}]*\}/g, '')
  .replace(/&nbsp;/g, '')
  .trim()

export const unnamedIconButtons = (source) => buttons(source)
  .filter(({ tag, content }) => !spoken(content)
    && !/\baria-label(?:ledby)?\s*=/.test(tag)
    && !/\{\.\.\.[^}]+\}/.test(tag))
  .map(({ tag }) => tag.replace(/\s+/g, ' ').slice(0, 120))

describe('icon-only button names', () => {
  it.each(COMPONENTS)('%s names every icon-only button', (file) => {
    expect(unnamedIconButtons(read(file))).toEqual([])
  })

  it('finds an icon-only button without a name and accepts each naming shape', () => {
    const source = [
      '<button type="button" onclick={() => close()}><LucideIcon name="x" /></button>',
      '<button type="button" aria-label="Close"><LucideIcon name="x" /></button>',
      '<button type="button">{label}</button>',
      '<button type="button"><LucideIcon name="copy" /><span class="visually-hidden">Copy</span></button>',
      '<button type="button" aria-labelledby="title">{@html icon}</button>',
      '<button type="button">{#if open}<LucideIcon name="chevron-up" />{:else}<LucideIcon name="chevron-down" />{/if}</button>',
      '<button type="button"><span onclick={() => toggle()}><LucideIcon name="x" /></span></button>',
    ].join('\n')
    expect(unnamedIconButtons(source)).toEqual([
      '<button type="button" onclick={() => close()}>',
      '<button type="button">',
      '<button type="button">',
    ])
  })
})
