import { describe, expect, it } from 'vitest'
import { runFailureRaw, runFailureText } from './run-failure.js'
import { testRunLine } from '../agents/agent-draft.js'

// The text an older runtime stored for an agent run whose OpenAI token expired,
// as the Notifications page showed it.
const STORED_EXPIRED = '{ "error": { "message": "Your authentication token has expir…'
const STORED_EXPIRED_FULL = 'The model could not complete this reply. No eligible model served this turn. No eligible model served this turn. { "error": { "message": "Your authentication token has expired. Please try signing in again.", "type": "invalid_request_error", "code": "token_expired", "param": null }, "status": 401 } openai/gpt-6-astra answered 401.'

describe('run failure text', () => {
  it('reads a stored expired-token answer as the step that fixes it and keeps the answer for Show log', () => {
    expect(runFailureText(STORED_EXPIRED)).toBe('The sign-in expired. Sign in again in Models.')
    expect(runFailureRaw(STORED_EXPIRED)).toEqual([STORED_EXPIRED])
    expect(runFailureText(STORED_EXPIRED_FULL)).toBe('The OpenAI sign-in expired. Sign in again in Models.')
    expect(runFailureRaw(STORED_EXPIRED_FULL)).toEqual([STORED_EXPIRED_FULL])
  })

  it('keeps the router\'s own credential sentence', () => {
    const routed = 'No eligible model served this turn. The OpenAI sign-in expired. Sign in again in Models. { "error": { "code": "token_expired" } } me answered 401.'
    expect(runFailureText(routed)).toBe('The OpenAI sign-in expired. Sign in again in Models.')
    expect(runFailureText('The Anthropic key was refused. Check it in Models.')).toBe('The Anthropic key was refused. Check it in Models.')
    expect(runFailureRaw('The OpenAI sign-in expired. Sign in again in Models.')).toEqual(['The OpenAI sign-in expired. Sign in again in Models.'])
  })

  it('names a provider\'s lasting refusal and keeps its answer for Show log', () => {
    const body = '{"code":"invalid-argument","error":"Client-side tools for multi-agent models require beta access"}'
    const routed = `The model could not complete this reply. No eligible model served this turn. xAI refused this model: it requires beta access. ${body} me answered 400.`
    expect(runFailureText(routed)).toBe('xAI refused this model: it requires beta access.')
    expect(runFailureRaw(routed)).toEqual([routed])
    const stored = `The model could not complete this reply. No eligible model served this turn. ${body} me answered 400.`
    expect(runFailureText(stored)).toBe('The provider refused this model: it requires beta access.')
    expect(runFailureRaw(stored)).toEqual([stored])
    expect(runFailureText(`xai/grok-4.20-multi-agent-0309: ${body}`)).toBe('xAI refused this model: it requires beta access.')
  })

  it('hides any other raw answer behind Show log and keeps the sentences before it once each', () => {
    const stored = 'The model could not complete this reply. No eligible model served this turn. No eligible model served this turn. {"error":{"message":"Overloaded","status":529}}'
    expect(runFailureText(stored)).toBe('The model could not complete this reply. No eligible model served this turn.')
    expect(runFailureRaw(stored)).toEqual([stored])
    expect(runFailureText('{"error":{"message":"The server is overloaded."}}')).toBe('The server is overloaded.')
    expect(runFailureText('{ "error": { "message": "Rate limit reach…')).toBe('The model could not complete this reply.')
  })

  it('names a route by its model and leaves a plain cause as it is', () => {
    expect(runFailureText('openai/gpt-6.1-sol: Every account for this provider is temporarily unavailable.'))
      .toBe('GPT-6.1 Sol: Every account for this provider is temporarily unavailable.')
    expect(runFailureText('The thread was not found.')).toBe('The thread was not found.')
    expect(runFailureRaw('The thread was not found.')).toEqual([])
    expect(runFailureText('The model could not complete this reply. No eligible model served this turn. The OpenAI sign-in exp…'))
      .toBe('The model could not complete this reply. No eligible model served this turn. The OpenAI sign-in exp…')
    expect(runFailureText(null)).toBe('')
    expect(runFailureText('  ')).toBe('')
  })

  it('names a refused model by its display name on every surface and keeps the raw id for Show log', () => {
    const refusal = 'The router does not serve the model xai/grok-4.20-multi-agent-0309.'
    expect(runFailureText(refusal)).toBe('The router does not serve the model Grok 4.20 Multi Agent 0309.')
    expect(runFailureRaw(refusal)).toEqual([refusal])
    expect(testRunLine({ status: 'failed', error: refusal }).text).toBe('Test run failed. The router does not serve the model Grok 4.20 Multi Agent 0309.')
    expect(runFailureText('anthropic/claude-sonnet-4-5 and openai/gpt-6.1-sol are busy.')).toBe('Claude Sonnet 4.5 and GPT-6.1 Sol are busy.')
  })

  it('leaves a file path and a word pair as they are', () => {
    expect(runFailureText('The file src/lib/run-failure.js and/or ~/docs/v1-notes could not be read.'))
      .toBe('The file src/lib/run-failure.js and/or ~/docs/v1-notes could not be read.')
    expect(runFailureRaw('The file src/lib/run-failure.js could not be read.')).toEqual([])
  })

  it('reads a failed test run in the agent draft the same way', () => {
    expect(testRunLine({ status: 'failed', error: STORED_EXPIRED }).text).toBe('Test run failed. The sign-in expired. Sign in again in Models.')
    expect(testRunLine({ status: 'failed' }).text).toBe('Test run failed.')
  })
})
