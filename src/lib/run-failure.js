import { modelDisplayName } from './provider-catalog.js'

// How every surface reads a run's recorded failure: a thread reply, a
// Notifications row, a Needs you item, an agent card, its history, a tooltip
// and a system notification. A stored text from an older runtime can repeat a
// sentence, name a route by its id, or carry a provider's raw answer cut short,
// so each surface shows the same sentence and keeps the raw answer for Show log.

// Strip known retry guidance, not punctuation inside file names or recorded causes.
const retryGuidance = new RegExp(`(?:^|(?<=[.!?])\\s)(?:${[
  'Try again',
  'Check the key and try again',
  'Check the files and try again',
  'Choose a smaller image before sending again',
  'Remove an image before sending again',
  'Remove images or choose smaller images before sending again',
  'Choose a PNG, JPEG, GIF, or WebP image before sending again',
].join('|')})\\.?$`, 'i')

// A runtime rejection the transport passes through as a log record, such as
// `code="invalid_request" reason="…"`. The UI shows a sentence for its code
// and keeps the record under Details.
const RAW_REJECTION = /\bcode="([a-z_]+)"/
const REJECTIONS = {
  invalid_request: 'The runtime rejected the request.',
  unauthorized: 'The runtime refused the request. Sign in again.',
  persistence_failed: 'The runtime could not save the request.',
  runtime_draining: 'The runtime is restarting. Try again shortly.',
}

// The router's sentences for a refused credential, as core reads them.
const SIGN_IN_EXPIRED = ' sign-in expired. Sign in again in Models.'
const KEY_REFUSED = ' key was refused. Check it in Models.'
// A provider's answer that its token expired, also when a stored text cuts
// it short inside the word.
const EXPIRED_TOKEN = /token_expired|token (?:has )?expir/i
const PROVIDERS = [[/\bopenai\b/i, 'OpenAI'], [/\banthropic\b/i, 'Anthropic'], [/\bxai\b/i, 'xAI']]

// Where a provider's raw JSON answer starts in a recorded text.
const RAW_BODY = /[{[]\s*"/
const BODY_MESSAGE = /"message"\s*:\s*"((?:[^"\\]|\\.)+)"/

function recordedText(value) {
  return typeof value === 'string' ? value.trim().replace(/\s+/g, ' ') : ''
}

// A reason that says one sentence twice, as a stored reply from an older
// router can, keeps the first of each.
function uniqueSentences(text) {
  const seen = new Set()
  return text.split(/(?<=[.!?])\s+/).filter((sentence) => {
    const key = sentence.toLowerCase()
    if (seen.has(key)) return false
    seen.add(key)
    return true
  }).join(' ')
}

// A failure names a model by its `family/model` id, such as the router's
// route prefix or its refusal of a model it does not serve. The text names
// the model as the picker does, so every surface reads the same. A file path
// or a word pair such as "and/or" stays: the model part carries a digit or a
// hyphen, and no slash, dot or word joins the id on either side.
const MODEL_ID = /(?<![\w./~-])([a-z][a-z0-9-]*)\/([a-z0-9][\w.:-]*[a-z0-9])(?![\w/])/gi
function isModelId(model) {
  return /[\d-]/.test(model)
}
function namedModels(text) {
  return text.replace(MODEL_ID, (id, family, model) => (isModelId(model) ? modelDisplayName(null, family, id) ?? id : id))
}
function namesModel(text) {
  return [...text.matchAll(MODEL_ID)].some(([, , model]) => isModelId(model))
}

// The sentence that names a refused credential and the step that fixes it:
// the router's own, else a provider's answer that its token expired.
function credentialStep(text) {
  for (const hint of [SIGN_IN_EXPIRED, KEY_REFUSED]) {
    const at = text.indexOf(hint)
    const start = at > 0 ? text.lastIndexOf('The ', at) : -1
    if (start >= 0) return `${text.slice(start, at)}${hint}`
  }
  if (!EXPIRED_TOKEN.test(text)) return ''
  const provider = PROVIDERS.find(([pattern]) => pattern.test(text))?.[1]
  return `The${provider ? ` ${provider}` : ''}${SIGN_IN_EXPIRED}`
}

// The sentence that names a provider's lasting refusal of the model: the
// router's own, such as `xAI refused this model: it requires beta access.`,
// else a provider's answer that the model requires beta access.
const MODEL_REFUSED = ' refused this model: '
function modelRefusal(text) {
  const at = text.indexOf(MODEL_REFUSED)
  if (at > 0) {
    const start = text.lastIndexOf('. ', at)
    const provider = text.slice(start < 0 ? 0 : start + 2, at).trim()
    const rest = text.slice(at + MODEL_REFUSED.length)
    const end = rest.search(/[{[]/)
    const cause = (end < 0 ? rest : rest.slice(0, end)).trim().replace(/\.$/, '')
    if (provider && cause) return `${provider}${MODEL_REFUSED}${cause}.`
  }
  if (!/beta access/i.test(text)) return ''
  const provider = PROVIDERS.find(([pattern]) => pattern.test(text))?.[1] ?? 'The provider'
  return `${provider}${MODEL_REFUSED}it requires beta access.`
}

// The text before a provider's raw answer, else the message the answer
// holds when it holds one whole.
function withoutBody(text) {
  const body = text.search(RAW_BODY)
  if (body < 0) return text
  const before = text.slice(0, body).trim()
  if (before) return before
  const message = BODY_MESSAGE.exec(text)?.[1]
  return message ? message.replace(/\\(.)/g, '$1') : 'The model could not complete this reply.'
}

// The sentence a recorded failure shows, or '' when it names no cause.
export function runFailureText(reason) {
  const recorded = recordedText(reason)
  if (!recorded) return ''
  const raw = RAW_REJECTION.exec(recorded)
  if (raw) return REJECTIONS[raw[1]] ?? 'The runtime rejected the request.'
  const step = credentialStep(recorded) || modelRefusal(recorded)
  if (step) return step
  const text = namedModels(uniqueSentences(withoutBody(recorded))).replace(retryGuidance, '').trim()
  if (!text) return ''
  return /[.!?…]$/.test(text) ? text : `${text}.`
}

// The recorded text Show log keeps when the sentence replaced part of it.
export function runFailureRaw(reason) {
  const recorded = recordedText(reason)
  if (!recorded) return []
  return RAW_REJECTION.test(recorded) || credentialStep(recorded) || modelRefusal(recorded) || RAW_BODY.test(recorded) || namesModel(recorded) ? [recorded] : []
}
