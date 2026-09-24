import hljs from 'highlight.js/lib/core'
import javascript from 'highlight.js/lib/languages/javascript'
import typescript from 'highlight.js/lib/languages/typescript'
import rust from 'highlight.js/lib/languages/rust'
import python from 'highlight.js/lib/languages/python'
import json from 'highlight.js/lib/languages/json'
import xml from 'highlight.js/lib/languages/xml'
import css from 'highlight.js/lib/languages/css'
import bash from 'highlight.js/lib/languages/bash'
import markdown from 'highlight.js/lib/languages/markdown'
import sql from 'highlight.js/lib/languages/sql'
import yaml from 'highlight.js/lib/languages/yaml'
for (const [name, language] of Object.entries({ javascript, typescript, rust, python, json, xml, css, bash, markdown, sql, yaml })) hljs.registerLanguage(name, language)
const extensions = { js: 'javascript', mjs: 'javascript', jsx: 'javascript', ts: 'typescript', tsx: 'typescript', rs: 'rust', py: 'python', json: 'json', html: 'xml', svg: 'xml', svelte: 'xml', css: 'css', sh: 'bash', bash: 'bash', md: 'markdown', sql: 'sql', yml: 'yaml', yaml: 'yaml' }
// Highlighting a large file blocks the window, so a file above this size shows as plain text.
export const HIGHLIGHT_LIMIT_BYTES = 200 * 1024
// A UTF-16 unit takes one to three UTF-8 bytes, so only a text near the limit is encoded.
const tooLarge = (text) => text.length > HIGHLIGHT_LIMIT_BYTES
  || (text.length * 3 > HIGHLIGHT_LIMIT_BYTES && new TextEncoder().encode(text).length > HIGHLIGHT_LIMIT_BYTES)
export function highlightFile(path, text) {
  const language = extensions[String(path).split('.').pop().toLowerCase()]
  if (!language || tooLarge(text)) return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
  return hljs.highlight(text, { language, ignoreIllegals: true }).value
}
