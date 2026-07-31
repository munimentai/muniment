// @vitest-environment jsdom

import { render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, describe, expect, it } from 'vitest'

import AssistantMarkdown from './AssistantMarkdown.svelte'

afterEach(() => document.body.replaceChildren())

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
})
