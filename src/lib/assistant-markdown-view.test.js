// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, describe, expect, it, vi } from 'vitest'

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

const frame = () => new Promise((resolve) => requestAnimationFrame(() => resolve()))

describe('assistant Markdown view', () => {
  it('places the caret at the end of the last text block and moves it as the text grows', async () => {
    const { container, rerender } = render(AssistantMarkdown, {
      props: { text: '## Heading\n\n- one\n- two', caret: true },
    })
    const caret = () => container.querySelector('.caret')
    expect(caret().parentElement).toHaveProperty('tagName', 'LI')
    expect(caret().parentElement).toHaveTextContent('two')

    await rerender({ text: '## Heading\n\n- one\n- two\n\n```js\nconst a = 1\n```', caret: true })
    await frame()
    expect(caret().parentElement).toHaveClass('assistant-markdown')
    expect(caret().previousElementSibling).toHaveProperty('tagName', 'PRE')

    await rerender({ text: '## Heading\n\nDone.', caret: true })
    await frame()
    expect(caret().parentElement).toHaveProperty('tagName', 'P')
    expect(container.querySelectorAll('.caret')).toHaveLength(1)

    await rerender({ text: '## Heading\n\nDone.', caret: false })
    expect(caret()).toBeNull()
  })

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

    await waitFor(() => expect(codeBlock).not.toHaveAttribute('tabindex'))
    expect(codeBlock).not.toHaveAttribute('role')
    expect(codeBlock).not.toHaveAttribute('aria-label')
    expect(table).not.toHaveAttribute('tabindex')
    expect(table).not.toHaveAttribute('role')
    expect(table).not.toHaveAttribute('aria-label')
  })

  it('draws streamed text once per frame and keeps the DOM of finished blocks', async () => {
    const { container, rerender } = render(AssistantMarkdown, {
      props: { text: 'First paragraph.\n\nSecond', caret: true },
    })
    const first = container.querySelector('p')
    const firstText = first.firstChild

    await rerender({ text: 'First paragraph.\n\nSecond par', caret: true })
    await rerender({ text: 'First paragraph.\n\nSecond paragraph', caret: true })
    expect(container.querySelectorAll('p')[1]).toHaveTextContent('Second')
    expect(container.querySelectorAll('p')[1]).not.toHaveTextContent('Second par')

    await frame()
    const paragraphs = container.querySelectorAll('p')
    expect(paragraphs).toHaveLength(2)
    expect(paragraphs[0]).toBe(first)
    expect(paragraphs[0].firstChild).toBe(firstText)
    expect(paragraphs[1]).toHaveTextContent('Second paragraph')
    expect(paragraphs[1].querySelector('.caret')).not.toBeNull()

    await rerender({ text: 'First paragraph.\n\nSecond paragraph.\n\n- item', caret: true })
    await frame()
    expect(container.querySelectorAll('p')[0]).toBe(first)
    expect(container.querySelector('li .caret')).not.toBeNull()
    expect(container.querySelectorAll('.caret')).toHaveLength(1)
  })

  it('opens a model link on a middle click and leaves other buttons to the link menu', async () => {
    const onopenlink = vi.fn()
    const { container } = render(AssistantMarkdown, {
      props: { text: '[Docs](https://example.com/docs)', onopenlink },
    })
    const link = container.querySelector('a')

    const middle = new MouseEvent('auxclick', { bubbles: true, cancelable: true, button: 1 })
    link.dispatchEvent(middle)
    expect(middle.defaultPrevented).toBe(true)
    await waitFor(() => expect(onopenlink).toHaveBeenCalledWith('https://example.com/docs'))

    const right = new MouseEvent('auxclick', { bubbles: true, cancelable: true, button: 2 })
    link.dispatchEvent(right)
    expect(right.defaultPrevented).toBe(false)
    expect(onopenlink).toHaveBeenCalledOnce()

    await fireEvent.click(link)
    await waitFor(() => expect(onopenlink).toHaveBeenCalledTimes(2))
  })
})
