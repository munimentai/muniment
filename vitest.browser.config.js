import { playwright } from '@vitest/browser-playwright'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  // Layout tests mount Svelte components in the real browser.
  plugins: [svelte()],
  resolve: {
    conditions: ['browser'],
  },
  test: {
    include: ['src/**/*.browser.test.js'],
    browser: {
      enabled: true,
      headless: true,
      provider: playwright({
        contextOptions: { viewport: { width: 600, height: 800 } },
      }),
      instances: [{ browser: 'chromium' }],
    },
  },
})
