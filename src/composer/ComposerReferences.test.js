import '@testing-library/jest-dom/vitest'
import { cleanup, render } from '@testing-library/svelte'
import { afterEach, expect, it } from 'vitest'
import ComposerReferences from './ComposerReferences.svelte'
afterEach(cleanup)
const files = [{ path: '/home/ISSUES.md', referenceName: 'ISSUES.md' }]
it('renders draft references without interpreting their HTML', () => {
  const view = render(ComposerReferences, { text: '<img onerror="alert(1)"> https://pi.dev @ISSUES.md', files })
  expect(view.container.querySelector('img')).toBeNull()
  expect(view.container.querySelectorAll('.reference')).toHaveLength(2)
})
it('keeps every character of the draft and covers the scheme and the @ with icons', () => {
  const text = 'See https://ollama.com/pricing and @ISSUES.md then /review'
  const view = render(ComposerReferences, { text, files, commands: ['review'] })
  const overlay = view.container.querySelector('.reference-overlay')
  const drawn = overlay.cloneNode(true)
  drawn.querySelectorAll('.mark').forEach((node) => node.remove())
  expect(drawn.textContent).toBe(`${text}\n`)
  const link = overlay.querySelector('.reference.link')
  expect(link).toHaveAttribute('data-url', 'https://ollama.com/pricing')
  expect(link.querySelector('.prefix')).toHaveTextContent('https://')
  expect(link.querySelector('.mark [data-icon="globe"]')).not.toBeNull()
  expect(overlay.querySelector('.reference .mark .file-icon').closest('.prefix')).toHaveTextContent('@')
  expect([...overlay.querySelectorAll('.reference')].find((node) => node.textContent === '/review').querySelector('.mark')).toBeNull()
  expect(overlay.querySelectorAll('.editing')).toHaveLength(0)
})
it('shows the hidden scheme again while the selection touches the link', async () => {
  const text = 'See https://ollama.com/pricing now'
  const view = render(ComposerReferences, { text, selection: { start: 12, end: 12 } })
  expect(view.container.querySelector('.reference.link')).toHaveClass('editing')
  await view.rerender({ text, selection: { start: text.length, end: text.length } })
  expect(view.container.querySelector('.reference.link')).not.toHaveClass('editing')
  await view.rerender({ text, selection: { start: 0, end: text.length } })
  expect(view.container.querySelector('.reference.link')).toHaveClass('editing')
})
it('marks only chosen files and named group members', () => {
  const agents = [{ id: 'bot', name: 'Research Bot' }]
  const view = render(ComposerReferences, { text: '@Research Bot @everyone @notes.md', agents })
  const marked = [...view.container.querySelectorAll('.reference')].map((node) => [node.classList.contains('agent'), !!node.querySelector('.mark')])
  expect(marked).toEqual([[true, true], [true, true], [false, false]])
  expect(view.container.querySelector('.reference.agent .agent-avatar')).not.toBeNull()
  expect(view.container.querySelectorAll('.reference.agent [data-icon="users"]')).toHaveLength(1)
})
it('finds the link under a point from its drawn boxes', () => {
  const view = render(ComposerReferences, { text: 'See https://ollama.com/pricing' })
  const link = view.container.querySelector('[data-url]')
  link.getClientRects = () => [{ left: 30, right: 200, top: 0, bottom: 20 }]
  const component = view.component
  expect(component.linkAt(40, 10)).toBe('https://ollama.com/pricing')
  expect(component.linkAt(10, 10)).toBeNull()
})
