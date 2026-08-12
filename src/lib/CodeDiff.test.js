// @vitest-environment jsdom

import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, describe, expect, it } from 'vitest'
import CodeDiff from './CodeDiff.svelte'

function textFile(path, text = 'const value = true') {
  return {
    oldPath: path,
    newPath: path,
    status: 'modified',
    binary: false,
    hunks: [{
      oldStart: 1,
      oldCount: 1,
      newStart: 1,
      newCount: 1,
      header: '@@ -1 +1 @@',
      lines: [{
        kind: 'context',
        oldLineNumber: 1,
        newLineNumber: 1,
        text,
        segments: [],
      }],
    }],
  }
}

function value(files, options = {}) {
  return {
    schemaVersion: 1,
    id: 'diff-view',
    truncated: false,
    files,
    ...options,
  }
}

afterEach(() => document.body.replaceChildren())

describe('code diff view', () => {
  it.each([
    ['added.json', 'new.txt added'],
    ['modified.json', 'src/message.txt changed'],
    ['deleted.json', 'old.txt deleted'],
    ['renamed.json', 'old-name.txt → new-name.txt renamed'],
  ])('renders the %s fixture header', (fixtureName, expectedHeader) => {
    const fixturePath = join(process.cwd(), 'protocol-fixtures/code-diff/1', fixtureName)
    const codeDiff = JSON.parse(readFileSync(fixturePath, 'utf8'))
    const { container } = render(CodeDiff, { props: { codeDiff } })
    const header = container.querySelector('.d2h-file-header')

    expect(header?.textContent.replace(/\s+/g, ' ').trim()).toBe(expectedHeader)
    expect(container).not.toHaveTextContent('Viewed')
  })

  it('renders text files through the side-by-side renderer', () => {
    const { container } = render(CodeDiff, { props: { codeDiff: value([textFile('src/file.js')]) } })

    expect(container.querySelector('.d2h-file-side-diff')).toBeInTheDocument()
    expect(container.querySelector('.code-diff')).toHaveAttribute('data-diff-id', 'diff-view')
    expect(container).toHaveTextContent('src/file.js')
    expect(container).toHaveTextContent('const value = true')
  })

  it('renders the warning before the empty state', () => {
    const { container } = render(CodeDiff, {
      props: { codeDiff: value([], { truncated: true }) },
    })

    expect(screen.getByRole('status')).toHaveTextContent('Warning: This diff is truncated.')
    expect(container.querySelector('.code-diff')?.children).toHaveLength(2)
    expect(container.querySelector('.code-diff')?.firstElementChild).toHaveClass('code-diff-warning')
    expect(container.querySelector('.code-diff')?.lastElementChild).toHaveTextContent('No changes.')
  })

  it('keeps text and binary records in contract order', () => {
    const { container } = render(CodeDiff, {
      props: {
        codeDiff: value([
          textFile('first.js'),
          { oldPath: 'asset-old.png', newPath: 'asset.png', status: 'renamed', binary: true, hunks: [] },
          textFile('last.js'),
        ]),
      },
    })
    const records = [...container.querySelectorAll('.code-diff-file, .code-diff-binary')]

    expect(records).toHaveLength(3)
    expect(records[0]).toHaveTextContent('first.js')
    expect(records[1]).toHaveTextContent('asset-old.png → asset.png')
    expect(records[1]).toHaveTextContent('Binary file changed')
    expect(records[2]).toHaveTextContent('last.js')
  })

  it('sanitizes renderer content before inserting it', () => {
    const attack = '<img src=x onerror="document.body.dataset.pwned = true">'
    const { container } = render(CodeDiff, {
      props: { codeDiff: value([textFile('unsafe.js', attack)]) },
    })

    expect(container.querySelector('img')).not.toBeInTheDocument()
    expect(container.querySelector('[onerror]')).not.toBeInTheDocument()
    expect(document.body).not.toHaveAttribute('data-pwned')
    expect(container).toHaveTextContent(attack)
  })

  it('makes overflowing diff panels keyboard accessible', async () => {
    let scrollWidth = 500
    let resize
    class ResizeObserver {
      constructor(callback) {
        resize = callback
      }
      observe() {}
      disconnect() {}
    }
    Object.defineProperties(HTMLElement.prototype, {
      scrollWidth: { configurable: true, get: () => scrollWidth },
      clientWidth: { configurable: true, get: () => 400 },
    })
    globalThis.ResizeObserver = ResizeObserver

    const { container } = render(CodeDiff, {
      props: { codeDiff: value([textFile('src/file.js')]) },
    })
    const panels = [...container.querySelectorAll('.d2h-file-side-diff')]

    expect(panels).toHaveLength(2)
    await waitFor(() => expect(panels[0]).toHaveAttribute('tabindex', '0'))
    for (const panel of panels) {
      expect(panel).toHaveAttribute('role', 'group')
      expect(panel).toHaveAccessibleName('Code diff panel')
    }

    scrollWidth = 400
    resize()

    for (const panel of panels) {
      expect(panel).not.toHaveAttribute('tabindex')
      expect(panel).not.toHaveAttribute('role')
      expect(panel).not.toHaveAttribute('aria-label')
    }
  })
})
