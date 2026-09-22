import { copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, readdirSync, realpathSync, renameSync, writeFileSync } from 'node:fs'
import { join, relative } from 'node:path'
const target = 'src-tauri/target/release'
const output = process.argv[2] || join(target,'cef-app')
for (const name of ['bootstrap.exe', 'muniment_desktop.dll', 'libcef.dll', 'chrome_elf.dll', 'icudtl.dat', 'v8_context_snapshot.bin', 'resources.pak', 'locales']) {
  if (!existsSync(join(target, name))) throw new Error(`Missing CEF Windows resource: ${name}`)
}
mkdirSync(output,{recursive:true})
for (const name of ['LICENSE.md','THIRD_PARTY_NOTICES.md','THIRD_PARTY_RUST_NOTICES.md']) copyFileSync(name,join(output,name))
for (const entry of readdirSync(target)) {
  if (/\.(dll|pak|bin|dat|json)$/.test(entry) && entry !== 'muniment_desktop.dll') copyFileSync(join(target,entry),join(output,entry))
}
cpSync(join(target,'locales'),join(output,'locales'),{recursive:true})
copyFileSync(join(target,'bootstrap.exe'),join(output,'muniment-desktop.exe'))
copyFileSync(join(target,'muniment_desktop.dll'),join(output,'muniment-desktop.dll'))
for (const name of ['muniment-runtime.exe','muniment-reader.exe']) {
  if(existsSync(join(target,name))) copyFileSync(join(target,name),join(output,name))
}
const asr='src-tauri/third-party/sherpa-onnx-v1.13.2/windows-x86_64'
for(const name of readdirSync(asr).filter(n=>n.endsWith('.dll'))) copyFileSync(join(asr,name),join(output,name))
writeFileSync(join(output,'muniment-desktop.exe.manifest'),`<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1"><application><supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/><maxversiontested Id="10.0.18362.0"/></application></compatibility>
<dependency><dependentAssembly><assemblyIdentity type="Win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/></dependentAssembly></dependency>
<trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges><requestedExecutionLevel level="asInvoker"/></requestedPrivileges></security></trustInfo>
</assembly>`)
const cefBuild = 'src-tauri/target/release/build'
const cefSource = readdirSync(cefBuild).filter(n => n.startsWith('cef-dll-sys-')).flatMap(n => {
  const out = join(cefBuild,n,'out')
  return existsSync(out) ? readdirSync(out).map(name => join(out,name,'CREDITS.html')).filter(existsSync) : []
})[0]
if (!cefSource) throw new Error('CEF third-party credits are missing')
copyFileSync(cefSource,join(output,'CEF-CREDITS.html'))
if (process.argv[3] === '--installer-config') {
  const configPath = process.argv[4]
  if (!configPath) throw new Error('An installer config path is required')
  const existing = new Set(Object.values(JSON.parse(readFileSync('src-tauri/tauri.windows.conf.json', 'utf8')).bundle.resources))
  const resources = {}
  for (const name of readdirSync(output)) {
    if (existing.has(name) || name === 'muniment_desktop.dll') continue
    if (/\.(dll|pak|bin|dat|json|manifest)$/.test(name) || name === 'locales' || name === 'CEF-CREDITS.html') {
      const file = join(output, name)
      const source = relative(realpathSync('src-tauri'), realpathSync(file)).replaceAll('\\', '/')
      resources[source + (name === 'locales' ? '/' : '')] = name + (name === 'locales' ? '/' : '')
    }
  }
  writeFileSync(configPath, JSON.stringify({ bundle: { resources } }, null, 2))
  // WiX keeps the source basename. Stage the hyphenated DLL and remove the
  // Cargo-named output from Tauri's automatic DLL collection.
  renameSync(join(target, 'muniment_desktop.dll'), join(output, 'muniment-desktop.dll'))
  // Tauri bundles this executable without rebuilding the Rust entry point.
  copyFileSync(join(output, 'muniment-desktop.exe'), join(target, 'muniment-desktop.exe'))
}
console.log(output)
