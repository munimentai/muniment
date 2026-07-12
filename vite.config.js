import { readFileSync } from 'node:fs'
import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

const pkg = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8'))

// Frontend lives in src/ (index.html is the Vite entry; test/smoke.sh asserts it).
// Output goes to dist/ at the repo root, which tauri.conf.json points at via
// build.frontendDist.
export default defineConfig({
  root: 'src',
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
  },
})
