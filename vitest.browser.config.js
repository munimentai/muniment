import { readFileSync } from 'node:fs'
import { playwright } from '@vitest/browser-playwright'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vitest/config'

const pkg = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8'))

// The factory sandbox and the CI runners cap CPUs with a cgroup quota that
// Node's os.availableParallelism() does not see.
const maxWorkers = process.env.FACTORY_CPUS ? { maxWorkers: Number.parseInt(process.env.FACTORY_CPUS, 10) } : {}

export default defineConfig({
  // Layout tests mount Svelte components in the real browser.
  plugins: [svelte()],
  // The window reads its version, as the app build defines it.
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
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
    ...maxWorkers,
  },
})
