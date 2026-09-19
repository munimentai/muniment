import '@testing-library/jest-dom/vitest'
import { cleanup, render } from '@testing-library/svelte'
import { afterEach, expect, it } from 'vitest'
import ComposerReferences from './ComposerReferences.svelte'
afterEach(cleanup)
it('renders draft references without interpreting their HTML', () => {
  const view = render(ComposerReferences, { text: '<img onerror="alert(1)"> https://pi.dev @ISSUES.md' })
  expect(view.container.querySelector('img')).toBeNull()
  expect(view.container.querySelectorAll('.reference')).toHaveLength(2)
})
