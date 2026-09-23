import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { readFile, writeFile, stat } from 'node:fs/promises'
import { execFileSync } from 'node:child_process'
import path from 'node:path'

const installedFile = process.env.MUNIMENT_UPDATE_PROOF_INSTALLED_FILE
const expectedHash = process.env.MUNIMENT_UPDATE_PROOF_SHA256
const expectedVersion = process.env.MUNIMENT_UPDATE_PROOF_VERSION
const sentinel = process.env.MUNIMENT_UPDATE_PROOF_SENTINEL
const evidence = process.env.MUNIMENT_UPDATE_PROOF_EVIDENCE
const homePath = process.env.MUNIMENT_UPDATE_PROOF_HOME
for (const file of [installedFile, sentinel, evidence, homePath]) assert(path.isAbsolute(file ?? ''), 'Proof paths must be absolute')
assert.match(expectedHash ?? '', /^[a-f0-9]{64}$/)
assert.match(expectedVersion ?? '', /^\d+\.\d+\.\d+$/)
const digest = async file => createHash('sha256').update(await readFile(file)).digest('hex')

describe('real signed application update', () => {
  it('installs the exact release bytes through the update control', async function () {
    this.timeout(600000)
    const beforeHash = await digest(installedFile)
    assert.notEqual(beforeHash, expectedHash, 'The target bytes were already installed')
    const sentinelHash = await digest(sentinel)
    const beforeVersion = await browser.execute(() => window.__TAURI__.app.getVersion())
    assert.equal(beforeVersion, '0.0.0', 'Use the isolated lower-version test baseline')
    await browser.waitUntil(async () => {
      const state = await browser.execute(() => window.__TAURI__.core.invoke('runtime_state'))
      return state.lastEvent === 'connected' && state.busy === false
    }, { timeout: 120000, timeoutMsg: 'The baseline runtime did not connect before the update test.' })
    const title = `Update proof ${Date.now()}`
    const threadId = await browser.execute(async (home, threadTitle) => {
      await window.__TAURI__.core.invoke('home_confirm', { homePath: home })
      const id = await window.__TAURI__.core.invoke('chat_new_thread')
      await window.__TAURI__.core.invoke('chat_rename_thread', { threadId: id, title: threadTitle })
      const result = await window.__TAURI__.core.invoke('chat_thread_summaries', { limit: 20 })
      if (!result.summaries.some(thread => thread.threadId === id && thread.title === threadTitle)) {
        throw new Error('The baseline did not persist the proof thread')
      }
      return id
    }, homePath, title)
    assert.equal(typeof threadId, 'string')
    assert.ok(threadId.length > 0)
    await browser.refresh()
    await $('[data-testid="local-mode"]').waitForDisplayed({ timeout: 120000 })
    const control = await $(`button[aria-label="Install update ${expectedVersion} and restart"]`)
    try { await control.waitForDisplayed({ timeout: 180000 }) }
    catch (failure) {
      await browser.setTimeout({ script: 150000 })
      const result = await browser.executeAsync(done => {
        window.__TAURI__.core.invoke('app_update_prepare').then(value => done({ value }), error => done({ error: String(error) }))
      })
      await writeFile(path.join(process.env.MUNIMENT_E2E_RAW_DIR, 'updater-error.json'), JSON.stringify(result))
      throw new Error(`Update control missing: ${JSON.stringify(result)}; ${String(failure)}`)
    }
    assert.equal(await control.getAttribute('aria-label'), `Install update ${expectedVersion} and restart`)
    const offeredVersion = expectedVersion
    assert.equal(await control.isEnabled(), true)
    const beforePid = Number(await readFile(process.env.MUNIMENT_UPDATE_PROOF_PID_FILE, 'utf8'))
    assert.ok(Number.isSafeInteger(beforePid) && beforePid > 1)
    const restartLogOffset = (await stat(process.env.MUNIMENT_E2E_DRIVER_APP_LOG)).size
    let clickError = null
    try { await control.click() } catch (error) { clickError = String(error) }
    // Driver disconnect is expected when the application exits. It is never proof
    // of installation. Only a match with the independently extracted release
    // hash establishes that the updater replaced this installed file.
    let afterHash = null
    const deadline = Date.now() + 180000
    while (Date.now() < deadline) {
      try { afterHash = await digest(installedFile) } catch {}
      if (afterHash === expectedHash) break
      await new Promise(resolve => setTimeout(resolve, 500))
    }
    assert.equal(afterHash, expectedHash, `Installed bytes did not match the release. Click error: ${clickError}`)
    assert.equal(await digest(sentinel), sentinelHash, 'The profile sentinel changed')
    await writeFile(evidence, JSON.stringify({ beforeVersion, offeredVersion, beforeHash, afterHash,
      sentinelHash, threadId, title, beforePid, restartLogOffset, storedThreadVerifiedAfterRestart: false, installedBytesMatch: true, sentinelPreserved: true,
      restartVerified: false, runtimeConnectionVerified: false, clickError }, null, 2), { mode: 0o600 })
    execFileSync('python3', ['/tmp/muniment-updater-install-proof/verify-macos.py'], { stdio: 'inherit', timeout: 240000 })
    // The verified production app has no embedded WebDriver server. Its
    // predecessor and session ended during the update, so do not DELETE it.
    browser.sessionId = undefined
    // The external runner must independently verify the restarted production
    // process, its connected runtime, its version and the profile's stored data.
    // This file alone does not declare the full updater acceptance test passed.
  })
})
