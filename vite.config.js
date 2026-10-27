import { readFileSync } from 'node:fs'
import { defineConfig } from 'vite'
import { configDefaults } from 'vitest/config'
import { svelte } from '@sveltejs/vite-plugin-svelte'

const pkg = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8'))

// The factory sandbox and the CI runners cap CPUs with a cgroup quota that
// Node's os.availableParallelism() does not see.
const maxWorkers = process.env.FACTORY_CPUS ? { maxWorkers: Number.parseInt(process.env.FACTORY_CPUS, 10) } : {}

// Frontend lives in src/ (index.html is the Vite entry; test/smoke.sh asserts it).
// Output goes to dist/ at the repo root, which tauri.conf.json points at via
// build.frontendDist.
export default defineConfig({
  root: 'src',
  publicDir: '../public',
  plugins: [svelte()],
  resolve: {
    conditions: ['browser'],
  },
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  // Tauri drives the lifecycle: fixed port so build.devUrl stays valid, and no
  // screen clearing so cargo output stays visible.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    outDir: '../dist',
    emptyOutDir: true,
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./test/isolated-home.setup.js', './test/model-registry.setup.js'],
    // The core tree runs its own node:test suites, and vitest.browser.config.js
    // runs the browser tests.
    exclude: [...configDefaults.exclude, 'core/**', '**/*.browser.test.js'],
    // Lazy component compilation and process fixtures share the CI worker budget.
    testTimeout: 30_000,
    ...maxWorkers,
  },
})
