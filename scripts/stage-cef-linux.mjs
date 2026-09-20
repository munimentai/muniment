import { copyFileSync, cpSync, existsSync, mkdirSync, readdirSync, rmSync } from 'node:fs'
import { join } from 'node:path'

const target = 'src-tauri/target/release'
const output = join(target, 'cef-resources')
rmSync(output, { recursive: true, force: true })
mkdirSync(output, { recursive: true })
for (const name of readdirSync(target)) {
  if (/\.(so(?:\.\d+)*|pak|bin|dat|json)$/.test(name) ||
      ['chrome-sandbox', 'chrome_crashpad_handler', 'muniment-cef-helper'].includes(name)) {
    copyFileSync(join(target, name), join(output, name))
  }
}
for (const name of ['libcef.so', 'icudtl.dat', 'chrome-sandbox', 'muniment-cef-helper']) {
  if (!existsSync(join(output, name))) throw new Error(`Missing CEF runtime file: ${name}`)
}
cpSync(join(target, 'locales'), join(output, 'locales'), { recursive: true })
const build = join(target, 'build')
const credits = readdirSync(build).filter(n => n.startsWith('cef-dll-sys-')).flatMap(n => {
  const out = join(build, n, 'out')
  return existsSync(out) ? readdirSync(out).map(n => join(out, n, 'CREDITS.html')).filter(existsSync) : []
})[0]
if (!credits) throw new Error('CEF third-party credits are missing')
copyFileSync(credits, join(output, 'CEF-CREDITS.html'))
