import DOMPurify from 'dompurify'
import { Marked } from 'marked'

const ALLOWED_TAGS = [
  'a', 'blockquote', 'br', 'code', 'del', 'em', 'h2', 'h3', 'h4', 'h5', 'hr',
  'li', 'ol', 'p', 'pre', 'strong', 'table', 'tbody', 'td', 'th', 'thead', 'tr', 'ul',
]
const ALLOWED_ATTR = ['class', 'href', 'rel', 'target', 'title']
const LINK_PROTOCOLS = new Set(['http:', 'https:', 'mailto:'])
const LANGUAGE = /^[A-Za-z0-9_+-]+$/u

function escapeHtml(value) {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;')
}

function approvedHref(href) {
  try {
    const url = new URL(href)
    return LINK_PROTOCOLS.has(url.protocol) ? url.href : null
  } catch {
    return null
  }
}

const renderer = {
  blockquote({ tokens }) {
    return `<blockquote>${this.parser.parse(tokens)}</blockquote>`
  },
  br() {
    return '<br>'
  },
  code({ text, lang }) {
    const language = lang?.trim().split(/\s+/u)[0]
    const className = language && LANGUAGE.test(language)
      ? ` class="language-${escapeHtml(language)}"`
      : ''
    return `<pre><code${className}>${escapeHtml(text)}</code></pre>`
  },
  codespan({ text }) {
    return `<code>${escapeHtml(text)}</code>`
  },
  def({ raw }) {
    return escapeHtml(raw)
  },
  del({ tokens }) {
    return `<del>${this.parser.parseInline(tokens)}</del>`
  },
  em({ tokens }) {
    return `<em>${this.parser.parseInline(tokens)}</em>`
  },
  heading(token) {
    if (token.depth > 4) return escapeHtml(token.raw)
    return `<h${token.depth + 1}>${this.parser.parseInline(token.tokens)}</h${token.depth + 1}>`
  },
  hr() {
    return '<hr>'
  },
  html({ raw }) {
    return escapeHtml(raw)
  },
  image({ text }) {
    return escapeHtml(text)
  },
  link({ href, title, tokens }) {
    const label = this.parser.parseInline(tokens)
    const safeHref = approvedHref(href)
    if (!safeHref) return label
    const safeTitle = title == null ? '' : ` title="${escapeHtml(title)}"`
    return `<a href="${escapeHtml(safeHref)}"${safeTitle} target="_blank" rel="noopener noreferrer">${label}</a>`
  },
  list(token) {
    if (token.items.some((item) => item.task)) return escapeHtml(token.raw)
    const tag = token.ordered ? 'ol' : 'ul'
    return `<${tag}>${token.items.map((item) => this.listitem(item)).join('')}</${tag}>`
  },
  listitem({ tokens }) {
    return `<li>${this.parser.parse(tokens)}</li>`
  },
  paragraph({ tokens }) {
    return `<p>${this.parser.parseInline(tokens)}</p>`
  },
  strong({ tokens }) {
    return `<strong>${this.parser.parseInline(tokens)}</strong>`
  },
  table(token) {
    const header = token.header
      .map((cell) => `<th>${this.parser.parseInline(cell.tokens)}</th>`)
      .join('')
    const body = token.rows
      .map((row) => `<tr>${row.map((cell) => `<td>${this.parser.parseInline(cell.tokens)}</td>`).join('')}</tr>`)
      .join('')
    return `<table><thead><tr>${header}</tr></thead><tbody>${body}</tbody></table>`
  },
  text({ text, tokens }) {
    return tokens ? this.parser.parseInline(tokens) : escapeHtml(text)
  },
}

const markdown = new Marked({
  async: false,
  gfm: true,
  renderer,
})

export function renderAssistantMarkdown(reply, {
  purifier = DOMPurify,
  reportDiagnostic = (message) => console.warn(message),
} = {}) {
  const text = typeof reply === 'string' ? reply : String(reply ?? '')
  if (!text) return { kind: 'text', text: '' }

  const parsed = markdown.parse(text)
  const html = purifier.sanitize(parsed, {
    ALLOWED_ATTR,
    ALLOWED_TAGS,
    ALLOW_DATA_ATTR: false,
    ALLOW_ARIA_ATTR: false,
  })

  // DOMPurify reports its internal fragment body, which did not come from the renderer.
  const removals = purifier.removed.filter(({ element }) => element?.nodeName !== 'BODY')
  if (removals.length > 0) {
    reportDiagnostic('Assistant Markdown sanitizer removed generated content.')
    return { kind: 'text', text }
  }

  return { kind: 'html', html }
}
