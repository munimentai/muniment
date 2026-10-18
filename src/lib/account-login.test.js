import { describe, expect, it } from 'vitest'
import { loginEnded, loginEvent, loginStart } from './account-login.js'

describe('account sign-in events', () => {
  const start = loginStart('openai-codex', 'ChatGPT')

  it('opens the browser on the sign-in page and finishes on done', () => {
    const browser = loginEvent(start, { provider: 'openai-codex', stage: 'event', event: { type: 'auth_url', url: 'https://auth.example/login' } })
    expect(browser.open).toBe('https://auth.example/login')
    expect(browser.login).toMatchObject({ stage: 'browser', url: 'https://auth.example/login' })
    const done = loginEvent(browser.login, { provider: 'openai-codex', stage: 'done', pool: true })
    expect(done).toMatchObject({ done: true, stop: true, pool: true })
    expect(loginEnded(done.login)).toBe(true)
  })

  it('shows a device code and opens its page', () => {
    const step = loginEvent(start, { stage: 'event', event: { type: 'device_code', userCode: 'ABCD-1234', verificationUri: 'https://device.example' } })
    expect(step.login).toMatchObject({ stage: 'code', code: 'ABCD-1234' })
    expect(step.open).toBe('https://device.example')
  })

  it('ignores another provider\'s events and fails an exit before done', () => {
    expect(loginEvent(start, { provider: 'kimi', stage: 'done' })).toBeNull()
    expect(loginEvent(start, { stage: 'exit' }).login.stage).toBe('failed')
  })
})
