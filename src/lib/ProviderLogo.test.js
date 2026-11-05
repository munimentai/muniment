import { afterEach, expect, it } from 'vitest'
import { cleanup, render } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ProviderLogo from './ProviderLogo.svelte'
afterEach(cleanup)
it('uses the Typesafe SVG for the unqualified Jev classifier name', () => {
  const {container} = render(ProviderLogo,{provider:'jev-latest',size:14})
  expect(container.querySelector('.blank')).toBeNull()
  expect(container.querySelector('.light svg')).not.toBeNull()
  expect(container.querySelector('.dark svg')).not.toBeNull()
})
it('shows the colored mark where color is asked for and the light and dark marks elsewhere', () => {
  const plain = render(ProviderLogo, { provider: 'anthropic' }).container
  expect(plain.querySelector('.light svg')).not.toBeNull()
  expect(plain.querySelector('title')?.textContent).not.toBe('Claude')
  cleanup()
  const colored = render(ProviderLogo, { provider: 'claude-bridge', color: true }).container
  expect(colored.querySelector('.light')).toBeNull()
  expect(colored.querySelector('title')?.textContent).toBe('Claude')
  cleanup()
  // A provider whose mark is one color keeps its light and dark marks.
  expect(render(ProviderLogo, { provider: 'openai', color: true }).container.querySelector('.dark svg')).not.toBeNull()
})
it('shows the oMLX mark for the oMLX server and its custom endpoint, and the generic mark for other endpoints', () => {
  const mark = (provider) => render(ProviderLogo, { provider }).container.querySelector('svg')?.outerHTML
  const omlx = mark('omlx')
  expect(omlx).toContain('omlx-tile')
  cleanup()
  expect(mark('custom-omlx')).toBe(omlx)
  cleanup()
  expect(mark('custom-omlx-2')).toBe(omlx)
  cleanup()
  const generic = mark('custom-my-litellm-proxy')
  expect(generic).not.toContain('omlx-tile')
  cleanup()
  expect(mark('custom-proxy')).toBe(generic)
})
it('draws a named endpoint as its monogram on a tile in a tint its id keeps, and keeps every built-in mark', () => {
  const tile = (provider, name) => render(ProviderLogo, { provider, name }).container.querySelector('.monogram')
  const doTest = tile('custom-do-test', 'DO Test')
  expect(doTest).toHaveTextContent('DT')
  expect(doTest.style.getPropertyValue('--tint')).toMatch(/^var\(--tint-(blue|green|amber|rose|teal|orange)\)$/)
  const tint = doTest.style.getPropertyValue('--tint')
  cleanup()
  expect(tile('custom-do-test', 'DO Test').style.getPropertyValue('--tint')).toBe(tint)
  cleanup()
  expect(tile('custom-live31', 'Live31')).toHaveTextContent('L3')
  cleanup()
  expect(tile('custom-proxy', 'proxy')).toHaveTextContent('P')
  cleanup()
  // A built-in provider, a server kind's mark and an unnamed endpoint keep their marks.
  for (const [provider, name] of [['openai', 'OpenAI'], ['custom-omlx', 'oMLX'], ['custom-proxy', '']]) {
    expect(tile(provider, name)).toBeNull()
    cleanup()
  }
})
