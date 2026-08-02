import { playwright } from '@vitest/browser-playwright'
import { defineConfig } from 'vitest/config'

export default defineConfig({
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
