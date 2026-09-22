// Display names only. Model IDs and provider keys remain unchanged on the wire.
const words = {
  openai: 'OpenAI', anthropic: 'Anthropic', typesafe: 'TypeSafe', xai: 'xAI',
  gpt: 'GPT', glm: 'GLM', jev: 'Jev', semif: 'SemIf', qwen: 'Qwen', gguf: 'GGUF',
  lmstudio: 'LM Studio', huggingface: 'Hugging Face', latest: 'Latest',
}

export function modelLabel(value) {
  if (value === undefined || value === null || value === '') return null
  return String(value).split('/').map(part => part
    .replace(/(claude-[a-z]+-)(\d+)-(\d+)(?=$|-)/gi, '$1$2.$3')
    .replace(/[-_]+/g, ' ')
    .replace(/\b(llama|qwen|granite|gemma|deepseek)(?=\d)/gi, '$1 ')
    .replace(/\b[a-z][a-z\d.]*/gi, word => words[word.toLowerCase()]
      ?? (/^qwen\d/i.test(word) ? `Qwen ${word.slice(4)}` : word[0].toUpperCase() + word.slice(1)))
    .replace(/\b(\d+(?:\.\d+)?)b\b/gi, '$1B')
  ).join(' · ')
}

// Exclusion messages carry a model ID before a colon and human-written reason.
export function modelReason(value) {
  const text = String(value)
  const boundary = text.indexOf(': ')
  return boundary < 0 ? text : `${modelLabel(text.slice(0, boundary))}: ${text.slice(boundary + 2)}`
}
