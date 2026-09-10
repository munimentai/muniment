import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const FILES = ['src/App.svelte', 'src/lib/Onboarding.svelte']

const root = process.cwd()
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8')
const markup = (source) => source
  .replace(/<script(?:\s[^>]*)?>[\s\S]*?<\/script>/g, '')
  .replace(/<style(?:\s[^>]*)?>[\s\S]*?<\/style>/g, '')
  .replace(/<!--[\s\S]*?-->/g, '')
const attribute = (element, name) => element.match(new RegExp(`\\b${name}\\s*=\\s*(["'])(.*?)\\1`, 's'))?.[2]
const hasAttribute = (element, name) => new RegExp(`\\b${name}\\s*=`).test(element)
const controls = (source) => {
  const elements = []
  const starts = source.matchAll(/<(?:input|textarea)\b/g)
  for (const start of starts) {
    let quote
    let braces = 0
    for (let index = start.index; index < source.length; index += 1) {
      const character = source[index]
      if (quote) {
        if (character === quote && source[index - 1] !== '\\') quote = undefined
      } else if (character === '"' || character === "'" || character === '`') quote = character
      else if (character === '{') braces += 1
      else if (character === '}') braces -= 1
      else if (character === '>' && braces === 0) {
        elements.push(source.slice(start.index, index + 1))
        break
      }
    }
  }
  return elements
}

const unnamedControls = (source, exception = () => false) => {
  const sourceMarkup = markup(source)
  const labelTargets = new Set(
    [...sourceMarkup.matchAll(/<label\b[^>]*\bfor\s*=\s*(["'])(.*?)\1[^>]*>/gs)]
      .map(([, , target]) => target),
  )

  return controls(sourceMarkup)
    .filter((element) => !hasAttribute(element, 'aria-label')
      && !hasAttribute(element, 'aria-labelledby')
      && !labelTargets.has(attribute(element, 'id'))
      && !exception(element))
}

describe('control names', () => {
  it.each(FILES)('%s names every input and textarea', (file) => {
    expect(unnamedControls(read(file))).toEqual([])
  })

  it('rejects each unsupported naming shape', () => {
    const source = '<label for="named">Name</label><input id="named"><input aria-labelledby="title"><textarea aria-label="Message"></textarea><input placeholder="Prompt"><textarea></textarea>'
    expect(unnamedControls(source)).toEqual(['<input placeholder="Prompt">', '<textarea>'])
  })

  it('names the first-run composer without a consent exception', () => {
    const source = read('src/lib/Onboarding.svelte')
    expect(source).toMatch(/<label[^>]*for="first-message">Message<\/label>/)
    expect(controls(markup(source))).toHaveLength(1)
    expect(unnamedControls(source)).toEqual([])
    expect(unnamedControls('<input type="checkbox">')).toEqual(['<input type="checkbox">'])
  })
})
