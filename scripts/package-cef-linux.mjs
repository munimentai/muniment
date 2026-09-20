import { copyFileSync, cpSync, existsSync, mkdirSync, readdirSync, rmSync } from 'node:fs'
import { join } from 'node:path'
const target='src-tauri/target/release'
const output=process.argv[2] || join(target,'cef-app')
mkdirSync(output,{recursive:true})
for (const name of ['THIRD_PARTY_NOTICES.md','THIRD_PARTY_RUST_NOTICES.md']) copyFileSync(name,join(output,name))
// A test run installs the official helper as root-owned and setuid. Replace it
// through its writable containing directory before packaging another build.
rmSync(join(output,'chrome-sandbox'),{force:true})
for(const name of readdirSync(target)) {
  if(/\.(so(?:\.\d+)*|pak|bin|dat|json)$/.test(name) || ['chrome-sandbox','chrome_crashpad_handler'].includes(name)) copyFileSync(join(target,name),join(output,name))
}
cpSync(join(target,'locales'),join(output,'locales'),{recursive:true})
for(const name of ['muniment-desktop','muniment-cef-helper','muniment-runtime','muniment-cli','muniment-reader']) {
  if(existsSync(join(target,name))) copyFileSync(join(target,name),join(output,name))
}
const asr='src-tauri/third-party/sherpa-onnx-v1.13.2/linux-x86_64'
for(const name of readdirSync(asr).filter(n=>n.includes('.so'))) copyFileSync(join(asr,name),join(output,name))
const cefBuild = 'src-tauri/target/release/build'
const cefSource = readdirSync(cefBuild).filter(n => n.startsWith('cef-dll-sys-')).flatMap(n => {
  const out = join(cefBuild,n,'out')
  return existsSync(out) ? readdirSync(out).map(name => join(out,name,'CREDITS.html')).filter(existsSync) : []
})[0]
if (!cefSource) throw new Error('CEF third-party credits are missing')
copyFileSync(cefSource,join(output,'CEF-CREDITS.html'))
console.log(output)
