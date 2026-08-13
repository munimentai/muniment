// @vitest-environment jsdom

import fs from 'node:fs'
import { describe, expect, it } from 'vitest'

const css = fs.readFileSync('src/styles/code-diff.css', 'utf8')
const wrapper = css.match(/\.code-diff \.d2h-wrapper\s*\{([^}]*)\}/)?.[1] ?? ''
const variables = Object.fromEntries([...wrapper.matchAll(/(--d2h-[\w-]+):\s*([^;]+);/g)]
  .map(([, name, value]) => [name, value]))

describe('code diff styles', () => {
  it('maps every diff2html color to a design token', () => {
    const packageColors = [
      'bg-color', 'border-color', 'dim-color', 'line-border-color',
      'file-header-bg-color', 'file-header-border-color',
      'empty-placeholder-bg-color', 'empty-placeholder-border-color',
      'selected-color', 'ins-bg-color', 'ins-border-color',
      'ins-highlight-bg-color', 'ins-label-color', 'del-bg-color',
      'del-border-color', 'del-highlight-bg-color', 'del-label-color',
      'change-del-color', 'change-ins-color', 'info-bg-color',
      'info-border-color', 'change-label-color', 'moved-label-color',
    ]

    for (const name of packageColors) {
      expect(variables[`--d2h-${name}`], name).toContain('var(--')
      expect(variables[`--d2h-dark-${name}`], `dark ${name}`).toContain('var(--')
    }
  })

  it('maps both change-row selectors to design tokens', () => {
    expect(css).toMatch(/\.d2h-file-diff \.d2h-del\.d2h-change\s*\{[^}]*var\(--oxide\)/)
    expect(css).toMatch(/\.d2h-file-diff \.d2h-ins\.d2h-change\s*\{[^}]*var\(--signal\)/)
  })

  it('stacks the side-by-side panels when the card is under 480 pixels wide', () => {
    expect(css).toMatch(/\.code-diff\s*\{[^}]*container-type:\s*inline-size;/)

    const narrow = css.match(/@container \(width < 480px\)\s*\{([\s\S]*)\}\s*$/)?.[1] ?? ''

    expect(narrow).toMatch(/\.d2h-files-diff\s*\{\s*display:\s*block;/)
    expect(narrow).toMatch(/\.d2h-file-side-diff\s*\{[^}]*width:\s*100%;/)
  })
})
