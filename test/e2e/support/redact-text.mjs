export const secrets = ['MUNIMENT_E2E_USERNAME', 'MUNIMENT_E2E_PASSWORD', 'GH_TOKEN']
  .map((name) => process.env[name]).filter(Boolean).sort((left, right) => right.length - left.length)
export const tokenPatterns = [
  ['credential-header', /\b(?:authorization|cookie|set-cookie)\s*[:=][^\r\n]+/gi],
  ['bearer-token', /\bBearer\s+[A-Za-z0-9._~+\/-]+=*/gi],
  ['oauth-token', /\b(?:access|refresh|id)_token\s*[:=]\s*[^\s,}\]]+/gi],
  ['github-token', /\bgh[opsu]_[A-Za-z0-9]{20,}\b/g],
  ['jwt', /\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b/g],
]

export function redactText(text) {
  for (const secret of secrets) text = text.split(secret).join('[REDACTED]')
  for (const [category, pattern] of tokenPatterns) text = text.replace(pattern, `[REDACTED:${category}]`)
  return text
}
