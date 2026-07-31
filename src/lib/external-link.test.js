import { fireEvent } from '@testing-library/svelte'
import { describe, expect, it, vi } from 'vitest'

import { createExternalLinkHandler } from './external-link.js'

function clickTarget(markup, selector = 'a') {
  const container = document.createElement('div')
  container.innerHTML = markup
  return { container, target: container.querySelector(selector) }
}

describe('external link handler', () => {
  it('cancels navigation and opens an approved link', async () => {
    const open = vi.fn().mockResolvedValue()
    const { container, target } = clickTarget('<a href="https://example.com/path"><span>Example</span></a>', 'span')
    container.addEventListener('click', createExternalLinkHandler(open))

    const allowed = await fireEvent.click(target)

    expect(allowed).toBe(false)
    expect(open).toHaveBeenCalledWith('https://example.com/path')
  })

  it('cancels navigation without opening a link whose protocol is not approved', async () => {
    const open = vi.fn()
    const { container, target } = clickTarget('<a href="ftp://example.com/file">Example</a>')
    container.addEventListener('click', createExternalLinkHandler(open))

    const allowed = await fireEvent.click(target)

    expect(allowed).toBe(false)
    expect(open).not.toHaveBeenCalled()
  })

  it('leaves a click outside a link unchanged', async () => {
    const open = vi.fn()
    const { container, target } = clickTarget('<button>Continue</button>', 'button')
    container.addEventListener('click', createExternalLinkHandler(open))

    const allowed = await fireEvent.click(target)

    expect(allowed).toBe(true)
    expect(open).not.toHaveBeenCalled()
  })

  it('catches a rejected open after it cancels navigation', async () => {
    const open = vi.fn().mockRejectedValue(new Error('unavailable'))
    const handler = createExternalLinkHandler(open)
    const { target } = clickTarget('<a href="mailto:user@example.com">Email</a>')
    const event = { target, preventDefault: vi.fn() }

    await expect(handler(event)).resolves.toBeUndefined()
    expect(event.preventDefault).toHaveBeenCalledOnce()
    expect(open).toHaveBeenCalledWith('mailto:user@example.com')
  })
})
