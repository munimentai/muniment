import { copyFileSync, cpSync, existsSync, mkdirSync, readdirSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'
import { LOCALE_ARCHIVE, LOCALE_MANIFEST, trimLocales } from './chromium-locales.mjs'

const target = 'src-tauri/target/release'
const isSharedLibrary = name => /\.so(?:\.\d+)*$/.test(name)
// The speech libraries ship once, in asr-runtime.
const speechLibrary = name => /^lib(?:onnxruntime|sherpa-onnx-c-api)\.so/.test(name)
const output = join(target, 'cef-resources')
rmSync(output, { recursive: true, force: true })
mkdirSync(output, { recursive: true })
// CEF's Linux libcef.so carries full debug symbols. Strip them in place so the
// package and linuxdeploy's AppImage copy both get the small library.
for (const name of readdirSync(target).filter(name => isSharedLibrary(name) && !speechLibrary(name))) {
  const result = spawnSync(process.env.STRIP || 'strip', ['--strip-unneeded', join(target, name)], { stdio: 'inherit' })
  if (result.error || result.status !== 0) throw new Error(`Cannot strip ${name}`)
}
for (const name of readdirSync(target)) {
  if (speechLibrary(name)) continue
  if (/\.(so(?:\.\d+)*|pak|bin|dat|json)$/.test(name) ||
      ['chrome-sandbox', 'chrome_crashpad_handler', 'muniment-cef-helper'].includes(name)) {
    copyFileSync(join(target, name), join(output, name))
  }
}
for (const name of ['libcef.so', 'icudtl.dat', 'chrome-sandbox', 'muniment-cef-helper']) {
  if (!existsSync(join(output, name))) throw new Error(`Missing CEF runtime file: ${name}`)
}
cpSync(join(target, 'locales'), join(output, 'locales'), { recursive: true })
trimLocales({
  directory: join(output, 'locales'),
  archive: join(target, 'bundle', 'locales', LOCALE_ARCHIVE),
  manifest: join(output, LOCALE_MANIFEST),
})
const build = join(target, 'build')
const credits = readdirSync(build).filter(n => n.startsWith('cef-dll-sys-')).flatMap(n => {
  const out = join(build, n, 'out')
  return existsSync(out) ? readdirSync(out).map(n => join(out, n, 'CREDITS.html')).filter(existsSync) : []
})[0]
if (!credits) throw new Error('CEF third-party credits are missing')
copyFileSync(credits, join(output, 'CEF-CREDITS.html'))
