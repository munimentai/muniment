// Display names only. Model IDs and provider keys remain unchanged on the wire.
const words = {
  openai: 'OpenAI', anthropic: 'Anthropic', typesafe: 'TypeSafe', xai: 'xAI',
  gpt: 'GPT', glm: 'GLM', jev: 'Jev', semif: 'SemIf', qwen: 'Qwen', gguf: 'GGUF',
  lmstudio: 'LM Studio', huggingface: 'Hugging Face', latest: 'Latest',
}

// `names` maps a provider id to the name the user gave it, so a custom
// endpoint reads as its own name, not its generated id.
export function modelLabel(value, names = null) {
  if (value === undefined || value === null || value === '') return null
  // Ollama's default tag, `:latest`, names no version, so the label leaves it off.
  const parts = String(value).replace(/:latest$/i, '').split('/')
  return parts.map((part, index) => index === 0 && parts.length > 1 && names?.[part] ? names[part] : part
    .replace(/(claude-[a-z]+-)(\d+)-(\d+)(?=$|-)/gi, '$1$2.$3')
    .replace(/[-_]+/g, ' ')
    .replace(/\b(llama|qwen|granite|gemma|deepseek)(?=\d)/gi, '$1 ')
    .replace(/\b[a-z][a-z\d.]*/gi, word => words[word.toLowerCase()]
      ?? (/^qwen\d/i.test(word) ? `Qwen ${word.slice(4)}` : word[0].toUpperCase() + word.slice(1)))
    .replace(/\b(\d+(?:\.\d+)?)b\b/gi, '$1B')
  ).join(' · ')
}

// `names` with the provider name a run's receipt carries for its own model,
// such as Anthropic for the Claude subscription bridge. A name the user gave
// the provider wins.
export function receiptNames(receipt, names = null) {
  const model = String(receipt?.model ?? '')
  const slash = model.indexOf('/')
  const name = typeof receipt?.provider_name === 'string' ? receipt.provider_name.trim() : ''
  return slash > 0 && name ? { [model.slice(0, slash)]: name, ...names } : names
}

// A picker row's name: the display name the registry or the server gives,
// else the model id itself without its folder and Ollama's `:latest` tag.
// An id keeps its own case, since title case mangles names such as
// `gpt-oss-20b-MXFP4-Q8`.
export function modelName(id, name = null) {
  const given = typeof name === 'string' ? name.trim() : ''
  if (given && given !== id) return given
  if (id === undefined || id === null || id === '') return null
  return String(id).replace(/:latest$/i, '').split('/').filter(Boolean).at(-1) ?? String(id)
}

// Exclusion messages carry a model ID before a colon and human-written reason.
export function modelReason(value) {
  const text = String(value)
  const boundary = text.indexOf(': ')
  return boundary < 0 ? text : `${modelLabel(text.slice(0, boundary))}: ${text.slice(boundary + 2)}`
}
