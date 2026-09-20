import { readdirSync, existsSync, mkdirSync, copyFileSync, writeFileSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'
const app = process.argv[2]
if (!app) throw new Error('Pass the app bundle path')
const build = 'src-tauri/target/release/build'
const candidates = readdirSync(build).filter(n => n.startsWith('cef-dll-sys-')).map(n => join(build,n,'out/cef_macos_aarch64'))
const source = candidates.find(p => existsSync(join(p,'include/cef_version.h')) && readFileSync(join(p,'include/cef_version.h'),'utf8').includes('152.0.6'))
if (!source) throw new Error('The pinned CEF framework is missing')
const frameworks=join(app,'Contents/Frameworks')
mkdirSync(frameworks,{recursive:true})
copyFileSync(join(source,'CREDITS.html'),join(app,'Contents/Resources/CEF-CREDITS.html'))
function run(cmd,args) { const r=spawnSync(cmd,args,{stdio:'inherit'}); if(r.status!==0) throw new Error(`${cmd} failed`) }
for (const executable of ['muniment-desktop','muniment-cef-helper']) {
  run('install_name_tool',['-add_rpath','@executable_path/../Frameworks',join(app,'Contents/MacOS',executable)])
}
run('ditto',[join(source,'Chromium Embedded Framework.framework'),join(frameworks,'Chromium Embedded Framework.framework')])
copyFileSync('src-tauri/target/release/libmuniment_cef_keychain.dylib',join(frameworks,'libmuniment_cef_keychain.dylib'))
for (const suffix of ['', ' (GPU)', ' (Renderer)', ' (Plugin)', ' (Alerts)']) {
  const name=`muniment CEF Helper${suffix}`
  const contents=join(frameworks,`${name}.app/Contents`)
  mkdirSync(join(contents,'MacOS'),{recursive:true})
  copyFileSync('src-tauri/target/release/muniment-cef-helper',join(contents,'MacOS',name))
  run('install_name_tool',['-add_rpath','@executable_path/../../..',join(contents,'MacOS',name)])
  const id=suffix.toLowerCase().replace(/[^a-z]/g,'') || 'main'
  writeFileSync(join(contents,'Info.plist'),`<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>${name}</string><key>CFBundleIdentifier</key><string>ai.muniment.desktop.cef.${id}</string><key>CFBundleName</key><string>${name}</string><key>CFBundlePackageType</key><string>APPL</string><key>LSUIElement</key><true/></dict></plist>`)
}
