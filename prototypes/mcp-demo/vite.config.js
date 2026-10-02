import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { fileURLToPath } from 'node:url'

export default defineConfig({
  plugins: [svelte()],
  publicDir: '../../public',
  resolve: { alias: { '@desktop': fileURLToPath(new URL('../../src', import.meta.url)) }, conditions: ['browser'] },
  server: { fs: { allow: [fileURLToPath(new URL('../..', import.meta.url))] } },
  build: { outDir: 'dist', emptyOutDir: true },
})
