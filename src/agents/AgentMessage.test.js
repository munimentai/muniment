import { afterEach, expect, it } from 'vitest'
import { cleanup, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import AgentMessage from './AgentMessage.svelte'
afterEach(cleanup)

it('renders an agent\'s result as Markdown, as replies render', () => {
  const envelope = { kind: 'message', from: 'Drive9 Helper', intent: 'result', hop: 2, text: '**Tauri 2**, according to the [official Tauri website](https://tauri.app).' }
  render(AgentMessage, { envelope })
  const message = screen.getByRole('article', { name: 'Message from Drive9 Helper' })
  expect(message.querySelector('strong:not(header strong)')).toHaveTextContent('Tauri 2')
  expect(screen.getByRole('link', { name: 'official Tauri website' })).toHaveAttribute('href', 'https://tauri.app/')
  expect(message).not.toHaveTextContent('**')
})
