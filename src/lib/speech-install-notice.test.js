import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createComposerNotices } from './composer-notices.js'
import { createSpeechInstallNotice, speechInstallProgress } from './speech-install-notice.js'

const installing = (completedBytes, totalBytes) => ({ state: 'installing', completedBytes, totalBytes })

function setup() {
  const notices = createComposerNotices()
  notices.attach()
  const cancel = vi.fn()
  const retry = vi.fn()
  const install = createSpeechInstallNotice({ notices, cancel, retry })
  return { notices, install, cancel, retry }
}

describe('speech install notice', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => {
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  it('reports bytes, and no maximum before the total is known', () => {
    expect(speechInstallProgress(installing(0, 0))).toEqual({ label: 'Speech model download progress', max: 0, value: 0, text: '' })
    expect(speechInstallProgress(installing(5 * 1024 * 1024, 641 * 1024 * 1024))).toMatchObject({ max: 641 * 1024 * 1024, value: 5 * 1024 * 1024, text: '5.0 MB of 641 MB' })
    expect(speechInstallProgress(installing(900, 100)).value).toBe(100)
  })

  it('stays at the right while the model downloads, then turns into the installed notice', () => {
    const { notices, install } = setup()
    install.update(installing(0, 0))
    const first = notices.current()
    expect(first).toMatchObject({ id: 'speech-install', placement: 'end', persistent: true, text: 'Downloading the speech model.' })
    expect(first.progress.max).toBe(0)
    install.update(installing(100, 1000))
    vi.advanceTimersByTime(120_000)
    install.update(installing(800, 1000))
    expect(notices.current()).toMatchObject({ cause: first.cause, progress: { value: 800, max: 1000 } })
    install.update({ state: 'installed' })
    expect(notices.current()).toMatchObject({ id: 'speech-install', placement: 'end', text: 'Installed. Press Voice again to dictate.' })
    expect(notices.current().persistent).toBeUndefined()
    expect(notices.current().progress).toBeUndefined()
    vi.advanceTimersByTime(6000)
    expect(notices.current()).toBeNull()
  })

  it('cancels the download from its action', () => {
    const { notices, install, cancel } = setup()
    install.update(installing(10, 100))
    expect(notices.current().action.label).toBe('Cancel')
    notices.act()
    expect(cancel).toHaveBeenCalledOnce()
    install.update({ state: 'cancelled' })
    expect(notices.current()).toBeNull()
  })

  it('turns a failed download into its failure with Try again', () => {
    const { notices, install, retry } = setup()
    install.update(installing(10, 100))
    install.update({ state: 'failed', message: 'The speech model download failed.' })
    expect(notices.current()).toMatchObject({ placement: 'end', text: 'The speech model download failed.', action: { label: 'Try again' } })
    notices.act()
    expect(retry).toHaveBeenCalledOnce()
    install.update(installing(0, 100))
    expect(notices.current()).toMatchObject({ persistent: true, cause: 'installing 2' })
  })

  it('shows nothing for a status it did not see download', () => {
    const { notices, install } = setup()
    install.update({ state: 'installed' })
    install.update({ state: 'notInstalled' })
    install.update(null)
    expect(notices.current()).toBeNull()
  })

  it('stays hidden once dismissed until the download ends', () => {
    const { notices, install } = setup()
    install.update(installing(10, 100))
    notices.dismiss()
    install.update(installing(50, 100))
    expect(notices.current()).toBeNull()
    install.update({ state: 'installed' })
    expect(notices.current()?.text).toBe('Installed. Press Voice again to dictate.')
  })

  it('withdraws the installed line when dictation starts, but not the download', () => {
    const { notices, install } = setup()
    install.update(installing(10, 100))
    install.clear()
    expect(notices.current()?.persistent).toBe(true)
    install.update({ state: 'installed' })
    install.clear()
    expect(notices.current()).toBeNull()
  })

  it('states a failed cancel on the download notice', () => {
    const { notices, install } = setup()
    install.update(installing(10, 100))
    install.update(installing(10, 100), 'The speech model install could not be cancelled. Try again.')
    expect(notices.current()).toMatchObject({ persistent: true, alert: true, text: 'The speech model install could not be cancelled. Try again.' })
    install.update(installing(20, 100))
    expect(notices.current()).toMatchObject({ alert: false, text: 'Downloading the speech model.' })
  })
})
