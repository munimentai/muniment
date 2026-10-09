import { createHash } from 'node:crypto'
import { mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { spawnSync } from 'node:child_process'

// The installer keeps Chromium's most used interface languages. Each release
// publishes the rest in one archive, and the manifest beside the bundled packs
// pins that archive's size and SHA-256 for the app's download in Preferences.
export const BUNDLED_LOCALES = ['ar', 'de', 'en-GB', 'en-US', 'es', 'es-419', 'fr', 'id', 'it', 'ja', 'ko', 'pt-BR', 'ru', 'zh-CN', 'zh-TW']
export const LOCALE_ARCHIVE = 'muniment-chromium-locales.tar.gz'
export const LOCALE_MANIFEST = 'chromium-locales.json'

// macOS names each pack's folder with underscores, and en.lproj holds en-US.
// Every platform carries a gendered variant of each pack beside it.
export function localeOf(name, macos) {
  const base = name.replace(/\.(lproj|pak)$/, '').replace(/_(FEMININE|MASCULINE|NEUTER)$/, '')
  return macos ? (base === 'en' ? 'en-US' : base.replaceAll('_', '-')) : base
}

// Move every pack outside the bundled set into the archive, then write the
// manifest. Returns the manifest.
export function trimLocales({ directory, macos = false, archive, manifest, run = spawnSync }) {
  const entries = readdirSync(directory).filter(name => name.endsWith(macos ? '.lproj' : '.pak')).sort()
  const codes = [...new Set(entries.map(name => localeOf(name, macos)))]
  const missing = BUNDLED_LOCALES.filter(code => !codes.includes(code))
  if (missing.length) throw new Error(`Chromium language packs are missing: ${missing.join(', ')}`)
  const removed = entries.filter(name => !BUNDLED_LOCALES.includes(localeOf(name, macos)))
  if (!removed.length) throw new Error('No Chromium language packs remain to archive')
  mkdirSync(dirname(archive), { recursive: true })
  rmSync(archive, { force: true })
  // tar ships with macOS, Linux and Windows 10 or later.
  const result = run('tar', ['-czf', archive, '-C', directory, ...removed], { stdio: 'inherit', env: { ...process.env, COPYFILE_DISABLE: '1' } })
  if (result.error || result.status !== 0) throw new Error('Cannot archive the Chromium language packs')
  for (const name of removed) rmSync(join(directory, name), { recursive: true, force: true })
  const value = {
    archive: LOCALE_ARCHIVE,
    sha256: createHash('sha256').update(readFileSync(archive)).digest('hex'),
    size: statSync(archive).size,
    bundled: BUNDLED_LOCALES,
    available: [...new Set(removed.map(name => localeOf(name, macos)))],
  }
  writeFileSync(manifest, `${JSON.stringify(value, null, 2)}\n`)
  return value
}
