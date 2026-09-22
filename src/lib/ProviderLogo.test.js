import { afterEach, expect, it } from 'vitest'
import { cleanup, render } from '@testing-library/svelte'
import ProviderLogo from './ProviderLogo.svelte'
afterEach(cleanup)
it('uses the Typesafe SVG for the unqualified Jev classifier name', () => {
  const {container} = render(ProviderLogo,{provider:'jev-latest',size:14})
  expect(container.querySelector('.blank')).toBeNull()
  expect(container.querySelector('.light svg')).not.toBeNull()
  expect(container.querySelector('.dark svg')).not.toBeNull()
})
