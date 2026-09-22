import { readdirSync, existsSync, mkdirSync, copyFileSync, writeFileSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'
const app = process.argv[2]
if (!app) throw new Error('Pass the app bundle path')
const universal = process.argv.includes('--universal')
const target = 'src-tauri/target'
const release = universal ? join(target, 'universal-apple-darwin/release') : join(target, 'release')
function cefSource(arch, triple) {
  const builds = [join(target, triple, 'release/build'), join(target, 'release/build')]
  const candidates = builds.filter(existsSync).flatMap(build => readdirSync(build)
    .filter(n => n.startsWith('cef-dll-sys-')).map(n => join(build,n,`out/cef_macos_${arch}`)))
  const source = candidates.find(p => existsSync(join(p,'include/cef_version.h')) && readFileSync(join(p,'include/cef_version.h'),'utf8').includes('152.0.6'))
  if (!source) throw new Error(`The pinned CEF framework is missing for ${arch}`)
  return source
}
const source = cefSource('aarch64', 'aarch64-apple-darwin')
const intel = universal ? cefSource('x86_64', 'x86_64-apple-darwin') : null
const frameworks=join(app,'Contents/Frameworks')
mkdirSync(frameworks,{recursive:true})
copyFileSync(join(source,'CREDITS.html'),join(app,'Contents/Resources/CEF-CREDITS.html'))
function run(cmd,args) { const r=spawnSync(cmd,args,{stdio:'inherit'}); if(r.status!==0) throw new Error(`${cmd} failed`) }
for (const executable of ['muniment-desktop','muniment-cef-helper']) {
  const binary = join(app,'Contents/MacOS',executable)
  const paths = spawnSync('otool', ['-l', binary], { encoding: 'utf8' })
  if (paths.status !== 0) throw new Error('Cannot inspect executable loader paths')
  if (!paths.stdout.includes('path @executable_path/../Frameworks (offset ')) {
    run('install_name_tool',['-add_rpath','@executable_path/../Frameworks',binary])
  }
}
run('ditto',[join(source,'Chromium Embedded Framework.framework'),join(frameworks,'Chromium Embedded Framework.framework')])
const frameworkName = 'Chromium Embedded Framework.framework'
const framework = join(frameworks, frameworkName)
const bridge = join(frameworks, 'libmuniment_cef_keychain.dylib')
if (universal) {
  // Keep both architecture-specific V8 snapshots, and merge every native library.
  for (const entry of readdirSync(join(intel, frameworkName, 'Resources'))) {
    if (/^v8_context_snapshot.*\.bin$/.test(entry)) {
      copyFileSync(join(intel, frameworkName, 'Resources', entry), join(framework, 'Resources', entry))
    }
  }
  const binaries = ['Chromium Embedded Framework', ...readdirSync(join(framework, 'Libraries'))
    .filter(name => name.endsWith('.dylib')).map(name => join('Libraries', name))]
  for (const binary of binaries) {
    run('lipo', ['-create', join(source, frameworkName, binary), join(intel, frameworkName, binary), '-output', join(framework, binary)])
    run('lipo', [join(framework, binary), '-verify_arch', 'arm64', 'x86_64'])
  }
  run('lipo', ['-create', ...['aarch64-apple-darwin', 'x86_64-apple-darwin']
    .map(triple => join(target, triple, 'release/libmuniment_cef_keychain.dylib')), '-output', bridge])
  run('lipo', [bridge, '-verify_arch', 'arm64', 'x86_64'])
} else copyFileSync(join(release, 'libmuniment_cef_keychain.dylib'), bridge)
run('install_name_tool', ['-change', '/System/Library/Frameworks/Security.framework/Versions/A/Security',
  '@loader_path/../libmuniment_cef_keychain.dylib', join(framework, 'Chromium Embedded Framework')])
for (const suffix of ['', ' (GPU)', ' (Renderer)', ' (Plugin)', ' (Alerts)']) {
  const name=`muniment CEF Helper${suffix}`
  const contents=join(frameworks,`${name}.app/Contents`)
  mkdirSync(join(contents,'MacOS'),{recursive:true})
  copyFileSync(join(release,'muniment-cef-helper'),join(contents,'MacOS',name))
  run('install_name_tool',['-add_rpath','@executable_path/../../..',join(contents,'MacOS',name)])
  const id=suffix.toLowerCase().replace(/[^a-z]/g,'') || 'main'
  writeFileSync(join(contents,'Info.plist'),`<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>${name}</string><key>CFBundleIdentifier</key><string>ai.muniment.desktop.cef.${id}</string><key>CFBundleName</key><string>${name}</string><key>CFBundlePackageType</key><string>APPL</string><key>LSUIElement</key><true/></dict></plist>`)
}
