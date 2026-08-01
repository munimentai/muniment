// @vitest-environment jsdom

import { render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, describe, expect, it } from 'vitest'

import AssistantMarkdown from './AssistantMarkdown.svelte'

const originalScrollWidth = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'scrollWidth')
const originalClientWidth = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientWidth')
const originalResizeObserver = Object.getOwnPropertyDescriptor(globalThis, 'ResizeObserver')

afterEach(() => {
  document.body.replaceChildren()
  restoreProperty(HTMLElement.prototype, 'scrollWidth', originalScrollWidth)
  restoreProperty(HTMLElement.prototype, 'clientWidth', originalClientWidth)
  restoreProperty(globalThis, 'ResizeObserver', originalResizeObserver)
})

function restoreProperty(target, name, descriptor) {
  if (descriptor) Object.defineProperty(target, name, descriptor)
  else delete target[name]
}

describe('assistant Markdown view', () => {
  it('inserts the sanitized HTML result', () => {
    const { container } = render(AssistantMarkdown, {
      props: { text: '## Heading\n\nA **strong** reply.' },
    })

    expect(screen.getByRole('heading', { name: 'Heading' })).toHaveProperty('tagName', 'H3')
    expect(container.querySelector('strong')).toHaveTextContent('strong')
  })

  it('renders empty text through a text node', () => {
    const { container } = render(AssistantMarkdown, { props: { text: '' } })
    const view = container.querySelector('.assistant-markdown')

    expect(view).toHaveTextContent('')
    expect(view.children).toHaveLength(0)
  })

  it('renders raw HTML source as text', () => {
    const { container } = render(AssistantMarkdown, {
      props: { text: '<script>alert("unsafe")</script>' },
    })

    expect(container.querySelector('script')).not.toBeInTheDocument()
    expect(container.querySelector('.assistant-markdown')).toHaveTextContent('<script>alert("unsafe")</script>')
  })

  it('adds and removes scroll region attributes as overflow changes', async () => {
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

    const { container } = render(AssistantMarkdown, {
      props: { text: '```sh\necho accessible\n```\n\n| Name | Value |\n| --- | --- |\n| one | two |' },
    })
    const codeBlock = container.querySelector('pre')
    const table = container.querySelector('table')

    await waitFor(() => expect(codeBlock).toHaveAttribute('tabindex', '0'))
    expect(codeBlock).toHaveAttribute('role', 'group')
    expect(codeBlock).toHaveAccessibleName('Code block')
    expect(table).toHaveAttribute('tabindex', '0')
    expect(table).toHaveAttribute('role', 'group')
    expect(table).toHaveAccessibleName('Table')

    scrollWidth = 400
    resize()

    expect(codeBlock).not.toHaveAttribute('tabindex')
    expect(codeBlock).not.toHaveAttribute('role')
    expect(codeBlock).not.toHaveAttribute('aria-label')
    expect(table).not.toHaveAttribute('tabindex')
    expect(table).not.toHaveAttribute('role')
    expect(table).not.toHaveAttribute('aria-label')
  })
})
