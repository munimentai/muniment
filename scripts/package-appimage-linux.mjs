import { cpSync, createReadStream, existsSync, lstatSync, mkdtempSync, readdirSync, readlinkSync, rmSync, symlinkSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'

// NSS loads host crypto modules and Wayland talks to the host compositor. Mixing
// their host and bundled library versions prevents browser startup on rolling distros.
export const hostLibrary = name => /^(?:lib(?:nss3|nssutil3|smime3|ssl3|nspr4|plc4|plds4)|libwayland-(?:client|cursor|egl|server))\.so(?:\..*)?$/.test(name)

export function prepareAppDir(appDir) {
  const libraryDir = join(appDir, 'usr/lib')
  const cef = join(libraryDir, 'muniment/cef')
  // Chromium resolves ICU, V8 and helper resources beside the loaded libcef,
  // which linuxdeploy places in usr/lib rather than Tauri's resource directory.
  for (const required of ['icudtl.dat', 'v8_context_snapshot.bin', 'resources.pak', 'locales']) {
    if (!existsSync(join(cef, required))) throw new Error(`Missing CEF AppImage resource: ${required}`)
  }
  // Keep linuxdeploy's patched libraries. Only Chromium data belongs here.
  for (const name of readdirSync(cef)) {
    if (name === 'icudtl.dat' || name === 'v8_context_snapshot.bin' || name === 'locales' || name.endsWith('.pak')) {
      cpSync(join(cef, name), join(libraryDir, name), { recursive: true })
    }
  }
  // CEF resolves the SUID helper beside libcef (DIR_ASSETS). An AppImage
  // cannot supply setuid permissions on its FUSE mount. Use the DEB-installed
  // helper on hosts that restrict user namespaces. Chromium validates it.
  const sandbox = join(libraryDir, 'chrome-sandbox')
  rmSync(sandbox, { force: true })
  symlinkSync('/usr/lib/muniment/cef/chrome-sandbox', sandbox)
  for (const name of readdirSync(libraryDir)) {
    if (hostLibrary(name)) rmSync(join(libraryDir, name))
  }
}

const digest = async file => {
  const hash = createHash('sha256')
  for await (const chunk of createReadStream(file)) hash.update(chunk)
  return hash.digest('hex')
}

export async function verifyTree(source, extracted) {
  if (readdirSync(source).sort().join('\n') !== readdirSync(extracted).sort().join('\n')) {
    throw new Error(`AppImage entries differ: ${source}`)
  }
  for (const name of readdirSync(source)) {
    const original = join(source, name), actual = join(extracted, name)
    const stat = lstatSync(original)
    const extractedStat = lstatSync(actual)
    if (stat.isSymbolicLink() !== extractedStat.isSymbolicLink() || stat.isDirectory() !== extractedStat.isDirectory() || stat.isFile() !== extractedStat.isFile()) {
      throw new Error(`AppImage entry type differs: ${original}`)
    }
    if (stat.isSymbolicLink()) {
      if (readlinkSync(original) !== readlinkSync(actual)) throw new Error(`AppImage link differs: ${original}`)
    } else if (stat.isDirectory()) {
      await verifyTree(original, actual)
    } else if (stat.isFile()) {
      if (await digest(original) !== await digest(actual)) throw new Error(`AppImage bytes differ: ${original}`)
      if ((stat.mode & 0o111) !== (lstatSync(actual).mode & 0o111)) throw new Error(`AppImage executable mode differs: ${original}`)
    }
  }
}

export async function packageAppImage(bundleDir, cacheDir, run = spawnSync) {
  const images = readdirSync(bundleDir).filter(name => name.endsWith('.AppImage'))
  if (images.length !== 1) throw new Error('Expected one AppImage to finalize')
  const appDir = join(bundleDir, 'muniment.AppDir')
  prepareAppDir(appDir)
  // The expanded image exceeds common /tmp tmpfs limits. Use the build disk.
  const work = mkdtempSync(join(bundleDir, '.muniment-appimage-'))
  const image = join(work, basename(images[0]))
  const execute = (command, args, options) => {
    const result = run(command, args, { stdio: 'inherit', ...options })
    if (result.error || result.status !== 0) throw new Error(`AppImage command failed: ${command}`)
  }
  try {
    execute('bash', [join(dirname(fileURLToPath(import.meta.url)), 'prepare-appimage-tool-linux.sh'), cacheDir])
    execute(join(cacheDir, 'muniment-appimage-tools-4.7.5/usr/bin/linuxdeploy-plugin-appimage'), ['--appdir', appDir], {
      env: { ...process.env, LDAI_OUTPUT: image },
    })
    execute(image, ['--appimage-extract'], { cwd: work, stdio: ['ignore', 'ignore', 'inherit'] })
    await verifyTree(appDir, join(work, 'squashfs-root'))
    cpSync(image, join(bundleDir, images[0]))
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const directory = resolve('src-tauri/target/release/bundle/appimage')
  if (existsSync(directory) && readdirSync(directory).some(name => name.endsWith('.AppImage'))) {
    const cache = join(process.env.XDG_CACHE_HOME || join(process.env.HOME, '.cache'), 'tauri')
    await packageAppImage(directory, cache)
  }
}
