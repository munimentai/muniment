import { describe, expect, it, vi } from 'vitest'

import DOMPurify from 'dompurify'

import { renderAssistantMarkdown, renderAssistantMarkdownBlocks } from './assistant-markdown.js'

function html(markdown) {
  const result = renderAssistantMarkdown(markdown)
  expect(result.kind).toBe('html')
  return result.html
}

describe('assistant Markdown', () => {
  it('returns an empty plain-text result for empty input', () => {
    expect(renderAssistantMarkdown('')).toEqual({ kind: 'text', text: '' })
  })

  it('renders every allowed element and attribute', () => {
    const result = html([
      '# One',
      '## Two',
      '### Three',
      '#### Four',
      '',
      'A **strong**, *emphasized*, ~~deleted~~ [link](https://example.com "Title"), and `code` line.  ',
      'Next line.',
      '',
      '> Quote',
      '',
      '---',
      '',
      '- unordered',
      '  - nested',
      '',
      '3. ordered',
      '',
      '```javascript',
      'const value = 1',
      '```',
      '',
      '| Head |',
      '| :---: |',
      '| Cell |',
    ].join('\n'))

    for (const tag of [
      'a', 'blockquote', 'br', 'code', 'del', 'em', 'h2', 'h3', 'h4', 'h5', 'hr',
      'li', 'ol', 'p', 'pre', 'strong', 'table', 'tbody', 'td', 'th', 'thead', 'tr', 'ul',
    ]) {
      expect(result).toContain(`<${tag}`)
    }
    expect(result).toContain('class="language-javascript"')
    expect(result).toContain('href="https://example.com/"')
    expect(result).not.toContain('title=')
    expect(result).toContain('target="_blank"')
    expect(result).toContain('rel="noopener noreferrer"')
    expect(result).not.toContain('start=')
    expect(result).not.toContain('align=')
  })

  it.each([
    ['raw HTML', '<aside>unsafe</aside>', '<aside', '<aside>unsafe</aside>'],
    ['a level-five heading', '##### Too deep', '<h6', '##### Too deep'],
    ['a task checkbox', '- [ ] undone', '<input', '- [ ] undone'],
    ['a footnote', 'Text[^1]\\n\\n[^1]: Note', '<sup', '[^1]'],
    ['a definition', '[term]: https://example.com', '<a', '[term]: https://example.com'],
  ])('renders %s as literal source without its element', (_name, source, forbidden, literal) => {
    const result = html(source.replaceAll('\\n', '\n'))

    expect(result).not.toContain(forbidden)
    expect(new DOMParser().parseFromString(result, 'text/html').body.textContent).toContain(literal)
  })

  it('renders image alt text without an image element', () => {
    const result = html('![remote description](https://example.com/tracker.png)')

    expect(result).toBe('<p>remote description</p>')
    expect(result).not.toContain('<img')
    expect(result).not.toContain('tracker.png')
  })

  it('omits an invalid code language class', () => {
    const result = html('```bad\" onclick=\"alert(1)\ncode\n```')

    expect(result).toBe('<pre><code>code</code></pre>')
    expect(result).not.toContain('class=')
  })

  it.each([
    ['https://example.com/path', 'https://example.com/path'],
    ['http://example.com/path', 'http://example.com/path'],
    ['mailto:person@example.com', 'mailto:person@example.com'],
  ])('allows the %s URL', (url, expected) => {
    const result = html(`[label](${url})`)

    expect(result).toContain(`<a href="${expected}"`)
    expect(result).toContain('target="_blank" rel="noopener noreferrer"')
  })

  it.each([
    ['relative', '/relative'],
    ['malformed', 'https://[invalid'],
    ['JavaScript', 'javascript:alert(1)'],
  ])('renders only the label for a %s URL', (_name, url) => {
    const result = html(`[safe label](${url})`)

    const text = new DOMParser().parseFromString(result, 'text/html').body.textContent
    expect(text).toBe('safe label')
    expect(result).not.toContain('<a')
    expect(result).not.toContain('href=')
  })

  it.each([
    ['whitespace in the destination', '[safe label](https:// example.com)', 'safe label'],
    ['a missing closing parenthesis', '[safe label](https://example.com', 'safe label'],
    ['a nested label', '[outer [inner]](https:// example.com)', 'outer [inner]'],
  ])('renders only the label for malformed link source with %s', (_name, source, label) => {
    const result = html(source)

    expect(new DOMParser().parseFromString(result, 'text/html').body.textContent)
      .toBe(label)
    expect(result).not.toContain('<a')
    expect(result).not.toContain('href=')
  })

  it.each([
    ['an inline code span', '`[safe](https:// example.com)`'],
    ['a fenced code block', '```\n[safe](https:// example.com)\n```'],
    ['escaped syntax', '\\[safe](https:// example.com)'],
    ['raw HTML', '<span>[safe](https:// example.com)</span>'],
  ])('preserves malformed link source inside %s', (_name, source) => {
    const result = html(source)

    expect(new DOMParser().parseFromString(result, 'text/html').body.textContent)
      .toContain('[safe](https:// example.com)')
  })

  it.each([
    ['a void HTML element', '<br> [safe](bad url)', '<br> safe'],
    ['an HTML comment', '<!-- x --> [safe](bad url)', '<!-- x --> safe'],
  ])('renders only the malformed link label after %s', (_name, source, expected) => {
    const result = html(source)

    expect(new DOMParser().parseFromString(result, 'text/html').body.textContent)
      .toBe(expected)
    expect(result).not.toContain('<a')
    expect(result).not.toContain('href=')
  })

  it.each([
    ['class on a paragraph', '<p class="language-js">changed</p>'],
    ['href on a heading', '<h2 href="https://example.com">changed</h2>'],
  ])('uses plain text when the sanitizer returns %s', (_name, sanitized) => {
    const reply = '**private reply text**'
    const reportDiagnostic = vi.fn()
    const purifier = {
      removed: [],
      sanitize: vi.fn(() => sanitized),
    }

    expect(renderAssistantMarkdown(reply, { purifier, reportDiagnostic })).toEqual({
      kind: 'text',
      text: reply,
    })
    expect(reportDiagnostic).toHaveBeenCalledOnce()
  })

  it('returns the complete source and records no source text when sanitization removes content', () => {
    const reply = '**private reply text**'
    const reportDiagnostic = vi.fn()
    const purifier = {
      removed: [{ element: document.createElement('script') }],
      sanitize: vi.fn(() => '<strong>changed</strong>'),
    }

    expect(renderAssistantMarkdown(reply, { purifier, reportDiagnostic })).toEqual({
      kind: 'text',
      text: reply,
    })
    expect(reportDiagnostic).toHaveBeenCalledOnce()
    expect(reportDiagnostic.mock.calls.flat().join(' ')).not.toContain(reply)
  })

  it('renders a reply as blocks that the whole-reply sanitizer leaves unchanged', () => {
    const reply = '## Title\n\nA [link](https://example.com).\n\n- one\n- two\n\n```js\ncode\n```\n\n| A |\n| - |\n| b |'
    const result = renderAssistantMarkdownBlocks(reply)
    const joined = result.blocks.map(({ html }) => html).join('')

    expect(result.kind).toBe('blocks')
    expect(result.blocks.map(({ raw }) => raw).join('')).toBe(reply)
    expect(DOMPurify.sanitize(joined, {
      ALLOWED_ATTR: ['class', 'href', 'rel', 'target', 'title'],
      ALLOWED_TAGS: ['a', 'code', 'h3', 'li', 'p', 'pre', 'table', 'tbody', 'td', 'th', 'thead', 'tr', 'ul'],
      ALLOW_DATA_ATTR: false,
      ALLOW_ARIA_ATTR: false,
    })).toBe(joined)
  })

  it('reuses the finished blocks of a growing reply and sanitizes only the rest', () => {
    const purifier = {
      get removed() { return DOMPurify.removed },
      sanitize: vi.fn((html, options) => DOMPurify.sanitize(html, options)),
    }
    const first = renderAssistantMarkdownBlocks('One.\n\nTwo', { purifier })
    purifier.sanitize.mockClear()

    const next = renderAssistantMarkdownBlocks('One.\n\nTwo and more', { purifier, previous: first })

    expect(next.blocks[0]).toBe(first.blocks[0])
    expect(next.blocks[1]).toBe(first.blocks[1])
    expect(next.blocks[2]).not.toBe(first.blocks[2])
    expect(next.blocks[2].html).toBe('<p>Two and more</p>')
    expect(purifier.sanitize).toHaveBeenCalledOnce()
    expect(purifier.sanitize).toHaveBeenCalledWith('<p>Two and more</p>', expect.any(Object))
  })

  it('renders every block again when a reference definition changes', () => {
    const first = renderAssistantMarkdownBlocks('See [docs].\n\nMore')
    const next = renderAssistantMarkdownBlocks('See [docs].\n\nMore\n\n[docs]: https://example.com', { previous: first })

    expect(next.blocks[0]).not.toBe(first.blocks[0])
    expect(next.blocks[0].html).toContain('<a href="https://example.com/"')
  })

  it('uses plain text for the whole reply when a new block fails the sanitizer check', () => {
    const reportDiagnostic = vi.fn()
    const first = renderAssistantMarkdownBlocks('One.\n\nTwo')
    const purifier = {
      removed: [],
      sanitize: vi.fn(() => '<p class="language-js">changed</p>'),
    }

    expect(renderAssistantMarkdownBlocks('One.\n\nTwo more', { purifier, reportDiagnostic, previous: first })).toEqual({
      kind: 'text',
      text: 'One.\n\nTwo more',
    })
    expect(reportDiagnostic).toHaveBeenCalledOnce()
  })
})
