// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import PhonePairing from './PhonePairing.svelte'
import Settings from './Settings.svelte'

vi.mock('../feature-flags.js', () => ({ featureFlags: { cloud: true, companyRecord: false } }))
import { PAIRING_POLL_MS, PAIRING_REPLACE_MS } from './pairing-state.js'
import { createPairingRequests, pairingRequests } from './pairing-requests.js'

vi.mock('./pairing-requests.js', async (original) => ({
  ...await original(),
  pairingRequests: { status: vi.fn(), challenge: vi.fn() },
}))

const phone = {
  pair_id: '22222222-2222-4222-8222-222222222222',
  mobile_device_id: '33333333-3333-4333-8333-333333333333',
  created_at: '2026-01-01T00:01:00.000Z',
}
const qrSvg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 21 21" role="img" aria-label="Pairing code"><rect width="21" height="21" fill="var(--surface)"/><path fill="currentColor" d="M0,0h1v1h-1z"/></svg>'

function renderPanel(invoke) {
  return render(PhonePairing, { props: { tauri: { invoke } } })
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(new Date('2026-01-01T00:00:00.000Z'))
  const requests = createPairingRequests()
  pairingRequests.status.mockImplementation(requests.status)
  pairingRequests.challenge.mockImplementation(requests.challenge)
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

function deferred() {
  let resolve
  let reject
  const promise = new Promise((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

function challengeResponse(seconds = 120) {
  return { expires_at: new Date(Date.now() + seconds * 1000).toISOString(), qr_svg: qrSvg }
}

async function openPairing() {
  const button = await screen.findByRole('button', { name: 'Pair a phone' })
  button.focus()
  await fireEvent.click(button)
  return button
}

function calls(invoke, command) {
  return invoke.mock.calls.filter(([name]) => name === command).length
}

describe('phone pairing', () => {
  it('traps Tab and Escape inside pairing and restores the Settings opener', async () => {
    const onclose = vi.fn()
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_pairing_status') return { pair: null }
      if (command === 'auth_pairing_challenge') return challengeResponse()
      if (command === 'auth_devices' || command === 'attach_companions') return []
      if (command === 'attach_listener_status') return { started: true, failure: null }
      if (command === 'thread_retention_choice') return 'keep_every_thread'
      if (command === 'auth_entitlement_snapshot') return { grants: [], capabilities: [], snapshot_version: 1 }
      throw new Error(`unexpected command: ${command}`)
    })
    render(Settings, { tauri: { invoke }, section: 'account', onclose })
    const opener = await openPairing()
    const close = await screen.findByRole('button', { name: 'Close pairing' })
    expect(close).toHaveFocus()
    await fireEvent.keyDown(close, { key: 'Tab' })
    expect(close).toHaveFocus()
    await fireEvent.keyDown(close, { key: 'Tab', shiftKey: true })
    expect(close).toHaveFocus()
    await fireEvent.keyDown(close, { key: 'Escape' })
    expect(screen.queryByRole('dialog', { name: 'Pair a phone' })).not.toBeInTheDocument()
    expect(screen.getByRole('dialog', { name: 'Settings' })).toBeInTheDocument()
    expect(onclose).not.toHaveBeenCalled()
    expect(opener).toHaveFocus()
    await fireEvent.keyDown(opener, { key: 'Escape' })
    expect(onclose).toHaveBeenCalledTimes(1)
  })
  it('spaces the initial read, retries, and replacement polls', async () => {
    const starts = []
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_pairing_challenge') return challengeResponse(1)
      starts.push(Date.now())
      if (starts.length === 1) throw new Error('offline')
      return { pair: null }
    })
    renderPanel(invoke)
    await fireEvent.click(await screen.findByRole('button', { name: 'Try again' }))
    await vi.advanceTimersByTimeAsync(1999)
    expect(starts).toHaveLength(1)
    await vi.advanceTimersByTimeAsync(1)
    await openPairing()
    await vi.advanceTimersByTimeAsync(10_000)
    await fireEvent.click(screen.getByRole('button', { name: 'New code' }))
    await vi.advanceTimersByTimeAsync(3000)
    expect(calls(invoke, 'auth_pairing_challenge')).toBe(2)
    expect(starts.length).toBeGreaterThan(2)
    expect(starts.slice(1).every((time, index) => time - starts[index] >= 2000)).toBe(true)
  })

  it('never overlaps slow status requests and cancels queued reads on close', async () => {
    const slow = deferred()
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_pairing_challenge') return challengeResponse()
      return calls(invoke, command) === 1 ? { pair: null } : slow.promise
    })
    renderPanel(invoke)
    await openPairing()
    await vi.advanceTimersByTimeAsync(12_000)
    expect(calls(invoke, 'auth_pairing_status')).toBe(2)
    await fireEvent.click(screen.getByRole('button', { name: 'Close pairing' }))
    slow.resolve({ pair: phone })
    await vi.advanceTimersByTimeAsync(20_000)
    expect(calls(invoke, 'auth_pairing_status')).toBe(2)
    expect(screen.queryByText(phone.mobile_device_id)).not.toBeInTheDocument()
  })

  it('retains status bounds across remounts and cancels a waiting initial read', async () => {
    const slow = deferred()
    const invoke = vi.fn(() => slow.promise)
    const first = renderPanel(invoke)
    first.unmount()
    const second = renderPanel(invoke)
    await vi.advanceTimersByTimeAsync(6000)
    expect(invoke).toHaveBeenCalledTimes(1)
    second.unmount()
    slow.resolve({ pair: phone })
    await vi.advanceTimersByTimeAsync(10_000)
    expect(invoke).toHaveBeenCalledTimes(1)
    expect(vi.getTimerCount()).toBe(0)
  })

  it('serializes repeated clicks and retains the cooldown after reopening and remounting', async () => {
    const slow = deferred()
    const invoke = vi.fn(async (command) => command === 'auth_pairing_status' ? { pair: null } : slow.promise)
    const first = renderPanel(invoke)
    const opener = await screen.findByRole('button', { name: 'Pair a phone' })
    await Promise.all([fireEvent.click(opener), fireEvent.click(opener)])
    expect(calls(invoke, 'auth_pairing_challenge')).toBe(1)
    await fireEvent.click(screen.getByRole('button', { name: 'Close pairing' }))
    slow.resolve(challengeResponse())
    await vi.advanceTimersByTimeAsync(0)
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    await openPairing()
    expect(await screen.findByRole('alert')).toHaveTextContent('Wait ten seconds')
    first.unmount()
    renderPanel(invoke)
    await vi.advanceTimersByTimeAsync(2000)
    await openPairing()
    expect(await screen.findByRole('alert')).toHaveTextContent('Wait ten seconds')
    expect(calls(invoke, 'auth_pairing_challenge')).toBe(1)
    await vi.advanceTimersByTimeAsync(8000)
    await fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
    expect(calls(invoke, 'auth_pairing_challenge')).toBe(2)
  })

  it('keeps a slow challenge serialized across remounts after the cooldown', async () => {
    const slow = deferred()
    const invoke = vi.fn(async (command) => command === 'auth_pairing_status' ? { pair: null } : slow.promise)
    const first = renderPanel(invoke)
    await openPairing()
    first.unmount()
    renderPanel(invoke)
    await vi.advanceTimersByTimeAsync(12_000)
    await openPairing()
    expect(await screen.findByRole('alert')).toHaveTextContent('Wait ten seconds')
    expect(calls(invoke, 'auth_pairing_challenge')).toBe(1)
    slow.resolve(challengeResponse())
    await vi.advanceTimersByTimeAsync(0)
    expect(screen.queryByRole('img')).not.toBeInTheDocument()
    await fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
    expect(calls(invoke, 'auth_pairing_challenge')).toBe(1)
    await vi.advanceTimersByTimeAsync(10_000)
    await fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
    expect(calls(invoke, 'auth_pairing_challenge')).toBe(2)
  })

  it('keeps polling a replacement when the expired challenge final read returns', async () => {
    const slow = deferred()
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_pairing_challenge') return challengeResponse(calls(invoke, command) === 1 ? 1 : 120)
      const count = calls(invoke, command)
      if (count === 1) return { pair: null }
      if (count === 2) return slow.promise
      return { pair: phone }
    })
    renderPanel(invoke)
    await openPairing()
    await vi.advanceTimersByTimeAsync(10_000)
    await fireEvent.click(screen.getByRole('button', { name: 'New code' }))
    slow.resolve({ pair: null })
    await vi.advanceTimersByTimeAsync(2000)
    expect(screen.getByText(phone.mobile_device_id)).toBeInTheDocument()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('ignores a challenge response after unmount and clears its timers', async () => {
    const slow = deferred()
    const invoke = vi.fn(async (command) => command === 'auth_pairing_status' ? { pair: null } : slow.promise)
    const view = renderPanel(invoke)
    await openPairing()
    view.unmount()
    slow.resolve(challengeResponse())
    await vi.advanceTimersByTimeAsync(20_000)
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(calls(invoke, 'auth_pairing_status')).toBe(1)
    expect(vi.getTimerCount()).toBe(0)
  })

  it('shows replacement failures inside the dialog', async () => {
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_pairing_status') return { pair: null }
      if (calls(invoke, command) === 1) return challengeResponse(1)
      throw new Error('private transport details')
    })
    renderPanel(invoke)
    await openPairing()
    await vi.advanceTimersByTimeAsync(10_000)
    await fireEvent.click(screen.getByRole('button', { name: 'New code' }))
    const alert = await screen.findByRole('alert')
    expect(screen.getByRole('dialog')).toContainElement(alert)
    expect(alert).toHaveTextContent('The pairing request failed.')
    expect(document.body).not.toHaveTextContent('private transport details')
  })

  it('removes a fractional-deadline QR without waiting for the countdown tick', async () => {
    const invoke = vi.fn(async (command) => command === 'auth_pairing_status' ? { pair: null } : challengeResponse(1.5))
    renderPanel(invoke)
    await openPairing()
    await vi.advanceTimersByTimeAsync(1499)
    expect(screen.getByRole('img')).toBeInTheDocument()
    await vi.advanceTimersByTimeAsync(1)
    expect(screen.queryByRole('img')).not.toBeInTheDocument()
    await vi.advanceTimersByTimeAsync(10_000)
    expect(calls(invoke, 'auth_pairing_status')).toBeLessThanOrEqual(3)
  })

  it('checks the deadline when the window resumes', async () => {
    const invoke = vi.fn(async (command) => command === 'auth_pairing_status' ? { pair: null } : challengeResponse(1.5))
    renderPanel(invoke)
    await openPairing()
    vi.setSystemTime(Date.now() + 1700)
    await fireEvent.focus(window)
    expect(screen.queryByRole('img')).not.toBeInTheDocument()
    expect(screen.getByText('The code expired.')).toBeInTheDocument()
  })

  it('accepts authorization that arrives after expiry and refreshes remote revocation', async () => {
    const slow = deferred()
    let pair = phone
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_pairing_challenge') return challengeResponse(2.5)
      const count = calls(invoke, command)
      if (count === 1) return { pair: null }
      if (count === 2) return slow.promise
      return { pair }
    })
    renderPanel(invoke)
    await openPairing()
    await vi.advanceTimersByTimeAsync(2700)
    expect(screen.queryByRole('img')).not.toBeInTheDocument()
    slow.resolve({ pair: phone })
    await vi.advanceTimersByTimeAsync(0)
    expect(screen.getByText(phone.mobile_device_id)).toBeInTheDocument()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    pair = null
    await vi.advanceTimersByTimeAsync(2000)
    expect(screen.getByText('No phone is paired.')).toBeInTheDocument()
    expect(screen.queryByText(phone.mobile_device_id)).not.toBeInTheDocument()
  })

  it('makes a final bounded read for authorization at the deadline', async () => {
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_pairing_challenge') return challengeResponse(1.5)
      return { pair: calls(invoke, command) > 1 ? phone : null }
    })
    renderPanel(invoke)
    await openPairing()
    await vi.advanceTimersByTimeAsync(2000)
    expect(screen.getByText(phone.mobile_device_id)).toBeInTheDocument()
  })
  it('shows the exact authorized phone after pairing', async () => {
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_pairing_status') return { pair: phone }
      throw new Error(`unexpected command: ${command}`)
    })
    renderPanel(invoke)
    expect(await screen.findByText('33333333-3333-4333-8333-333333333333')).toBeInTheDocument()
    expect(screen.getByText(/Paired phone/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Revoke phone' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Pair a phone' })).not.toBeInTheDocument()
  })

  it('encodes the returned qr object and expires at the server deadline', async () => {
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_pairing_status') return { pair: null }
      if (command === 'auth_pairing_challenge') return {
        expires_at: '2026-01-01T00:02:00.000Z',
        qr_svg: qrSvg,
      }
      throw new Error(`unexpected command: ${command}`)
    })
    renderPanel(invoke)
    await fireEvent.click(await screen.findByRole('button', { name: 'Pair a phone' }))
    expect(await screen.findByRole('img', { name: 'Pairing code' })).toBeInTheDocument()
    expect(screen.getByText('Code expires in 02:00')).toBeInTheDocument()
    expect(document.querySelector('.pairing-qr svg')).toHaveAttribute('aria-label', 'Pairing code')
    expect(document.querySelector('.pairing-qr path')).toHaveAttribute('d', 'M0,0h1v1h-1z')
    expect(document.body.textContent).not.toContain('access_token')
    expect(document.body.textContent).not.toContain('https://')

    await vi.advanceTimersByTimeAsync(120_000)
    expect(await screen.findByText('The code expired.')).toBeInTheDocument()
    expect(screen.queryByRole('img', { name: 'Pairing code' })).not.toBeInTheDocument()
  })

  it('polls at most every two seconds until the phone appears', async () => {
    let statusCalls = 0
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_pairing_challenge') return {
        expires_at: '2026-01-01T00:02:00.000Z',
        qr_svg: qrSvg,
      }
      if (command === 'auth_pairing_status') {
        statusCalls += 1
        return { pair: statusCalls >= 4 ? phone : null }
      }
      throw new Error(`unexpected command: ${command}`)
    })
    renderPanel(invoke)
    await fireEvent.click(await screen.findByRole('button', { name: 'Pair a phone' }))
    await screen.findByRole('img', { name: 'Pairing code' })
    const afterOpen = statusCalls
    expect(afterOpen).toBe(1)
    await vi.advanceTimersByTimeAsync(PAIRING_POLL_MS - 1)
    expect(statusCalls).toBe(afterOpen)
    await vi.advanceTimersByTimeAsync(1)
    expect(statusCalls).toBe(afterOpen + 1)
    await vi.advanceTimersByTimeAsync(PAIRING_POLL_MS)
    expect(statusCalls).toBe(3)
    expect(screen.queryByText(phone.mobile_device_id)).not.toBeInTheDocument()
    await vi.advanceTimersByTimeAsync(PAIRING_POLL_MS)
    expect(await screen.findByText('33333333-3333-4333-8333-333333333333')).toBeInTheDocument()
    expect(screen.queryByRole('dialog', { name: 'Pair a phone' })).not.toBeInTheDocument()
  })

  it('waits ten seconds before replacing an expired code', async () => {
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_pairing_status') return { pair: null }
      if (command === 'auth_pairing_challenge') return {
        expires_at: '2026-01-01T00:00:01.000Z',
        qr_svg: qrSvg,
      }
      throw new Error(`unexpected command: ${command}`)
    })
    renderPanel(invoke)
    await fireEvent.click(await screen.findByRole('button', { name: 'Pair a phone' }))
    await screen.findByRole('img', { name: 'Pairing code' })
    await vi.advanceTimersByTimeAsync(1000)
    expect(await screen.findByText('The code expired.')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'New code' })).toBeDisabled()
    await vi.advanceTimersByTimeAsync(PAIRING_REPLACE_MS)
    expect(screen.getByRole('button', { name: 'New code' })).toBeEnabled()
  })

  it('ignores stale status after local revocation', async () => {
    const slow = deferred()
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_pairing_revoke') return { revoked: true }
      return calls(invoke, command) === 1 ? { pair: phone } : slow.promise
    })
    renderPanel(invoke)
    await screen.findByText(phone.mobile_device_id)
    await vi.advanceTimersByTimeAsync(2000)
    await fireEvent.click(screen.getByRole('button', { name: 'Revoke phone' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Revoke' }))
    slow.resolve({ pair: phone })
    await vi.advanceTimersByTimeAsync(4000)
    expect(screen.getByText('No phone is paired.')).toBeInTheDocument()
    expect(calls(invoke, 'auth_pairing_status')).toBe(2)
  })

  it('keeps the phone after a failed revoke and supports retry', async () => {
    const invoke = vi.fn(async (command) => {
      if (command === 'auth_pairing_status') return { pair: phone }
      if (calls(invoke, command) === 1) throw new Error('private response')
      return { revoked: true }
    })
    renderPanel(invoke)
    await fireEvent.click(await screen.findByRole('button', { name: 'Revoke phone' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Revoke' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('The phone could not be revoked.')
    await vi.advanceTimersByTimeAsync(2000)
    expect(screen.queryByText('No phone is paired.')).not.toBeInTheDocument()
    await fireEvent.click(screen.getByRole('button', { name: 'Retry revoke' }))
    expect(await screen.findByText('No phone is paired.')).toBeInTheDocument()
  })

  it('revokes the pair and clears the authorized phone', async () => {
    let pair = phone
    const invoke = vi.fn(async (command, args) => {
      if (command === 'auth_pairing_status') return { pair }
      if (command === 'auth_pairing_revoke') {
        expect(args).toEqual({ pairId: phone.pair_id })
        pair = null
        return { revoked: true }
      }
      throw new Error(`unexpected command: ${command}`)
    })
    renderPanel(invoke)
    await fireEvent.click(await screen.findByRole('button', { name: 'Revoke phone' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Revoke' }))
    expect(await screen.findByText('No phone is paired.')).toBeInTheDocument()
    expect(invoke).toHaveBeenCalledWith('auth_pairing_revoke', { pairId: phone.pair_id })
  })
})
