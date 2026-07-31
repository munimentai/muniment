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

function malformedLinkSource(source) {
  if (source[0] !== '[') return null

  let bracketDepth = 1
  let labelEnd = -1
  for (let index = 1; index < source.length; index += 1) {
    if (source[index] === '\\') {
      index += 1
    } else if (source[index] === '[') {
      bracketDepth += 1
    } else if (source[index] === ']') {
      bracketDepth -= 1
      if (bracketDepth === 0) {
        labelEnd = index
        break
      }
    } else if (source[index] === '\n') {
      return null
    }
  }

  if (labelEnd < 1 || source[labelEnd + 1] !== '(') return null

  const destinationStart = labelEnd + 2
  const lineEnd = source.indexOf('\n', destinationStart)
  const closing = source.indexOf(')', destinationStart)
  const destinationEnd = closing < 0 || (lineEnd >= 0 && lineEnd < closing)
    ? (lineEnd < 0 ? source.length : lineEnd)
    : closing
  const destination = source.slice(destinationStart, destinationEnd)
  if (closing >= 0 && closing < destinationEnd) return null
  if (
    closing >= 0
    && /^\S+(?:\s+(?:"[^"]*"|'[^']*'|\([^)]*\)))?\s*$/u.test(destination)
  ) return null

  return {
    label: source.slice(1, labelEnd),
    raw: source.slice(0, closing >= 0 && closing < (lineEnd < 0 ? source.length : lineEnd)
      ? closing + 1
      : destinationEnd),
  }
}

const malformedLink = {
  name: 'malformedLink',
  level: 'inline',
  renderer(token) {
    return this.parser.parseInline(token.tokens)
  },
}

const markdown = new Marked({
  async: false,
  extensions: [malformedLink],
  gfm: true,
  renderer,
})

function splitMalformedLinks(token) {
  const tokens = []
  let rest = token.raw

  while (rest) {
    const start = rest.indexOf('[')
    if (start < 0) break
    const match = malformedLinkSource(rest.slice(start))
    if (!match || (start > 0 && rest[start - 1] === '!')) {
      const end = start + 1
      tokens.push({ type: 'text', raw: rest.slice(0, end), text: rest.slice(0, end) })
      rest = rest.slice(end)
      continue
    }
    if (start > 0) {
      tokens.push({ type: 'text', raw: rest.slice(0, start), text: rest.slice(0, start) })
    }
    tokens.push({
      type: 'malformedLink',
      raw: match.raw,
      tokens: markdown.Lexer.lexInline(match.label),
    })
    rest = rest.slice(start + match.raw.length)
  }

  if (rest) tokens.push({ type: 'text', raw: rest, text: rest })
  return tokens.length > 0 ? tokens : [token]
}

function rewriteMalformedLinks(tokens) {
  const rewritten = []
  const htmlStack = []

  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index]
    if (token.type === 'html' && !token.block) {
      const closing = token.raw.match(/^<\/([A-Za-z][\w-]*)/u)
      const opening = token.raw.match(/^<([A-Za-z][\w-]*)\b/u)
      if (closing) htmlStack.pop()
      rewritten.push(token)
      if (opening && !/\/>\s*$/u.test(token.raw)) htmlStack.push(opening[1].toLowerCase())
      continue
    }

    if (token.type === 'text' && htmlStack.length === 0) {
      const start = token.raw.indexOf('[')
      const combined = tokens.slice(index).map(({ raw }) => raw).join('')
      const match = start >= 0 ? malformedLinkSource(combined.slice(start)) : null
      if (match && start + match.raw.length > token.raw.length) {
        const consumedLength = start + match.raw.length
        let coveredLength = 0
        let lastIndex = index
        while (lastIndex < tokens.length && coveredLength < consumedLength) {
          coveredLength += tokens[lastIndex].raw.length
          lastIndex += 1
        }
        if (coveredLength === consumedLength) {
          if (start > 0) {
            rewritten.push({ type: 'text', raw: token.raw.slice(0, start), text: token.raw.slice(0, start) })
          }
          rewritten.push({
            type: 'malformedLink',
            raw: match.raw,
            tokens: markdown.Lexer.lexInline(match.label),
          })
          index = lastIndex - 1
          continue
        }
      }
      rewritten.push(...splitMalformedLinks(token))
      continue
    }

    if (Array.isArray(token.tokens)) rewriteMalformedLinks(token.tokens)
    rewritten.push(token)
  }

  tokens.splice(0, tokens.length, ...rewritten)
  return tokens
}

function hasForbiddenAttributes(html) {
  const template = document.createElement('template')
  template.innerHTML = html

  return [...template.content.querySelectorAll('*')].some((element) =>
    [...element.attributes].some(({ name, value }) => {
      if (element.localName === 'a') {
        return !['href', 'rel', 'target', 'title'].includes(name)
      }
      if (element.localName === 'code' && name === 'class') {
        return !value.split(/\s+/u).every((item) => /^language-[A-Za-z0-9_+-]+$/u.test(item))
      }
      return true
    }),
  )
}

export function renderAssistantMarkdown(reply, {
  purifier = DOMPurify,
  reportDiagnostic = (message) => console.warn(message),
} = {}) {
  const text = typeof reply === 'string' ? reply : String(reply ?? '')
  if (!text) return { kind: 'text', text: '' }

  const parsed = markdown.parser(rewriteMalformedLinks(markdown.lexer(text)))
  const html = purifier.sanitize(parsed, {
    ALLOWED_ATTR,
    ALLOWED_TAGS,
    ALLOW_DATA_ATTR: false,
    ALLOW_ARIA_ATTR: false,
  })

  // DOMPurify reports its internal fragment body, which did not come from the renderer.
  const removals = purifier.removed.filter(({ element }) => element?.nodeName !== 'BODY')
  if (removals.length > 0 || hasForbiddenAttributes(html)) {
    reportDiagnostic('Assistant Markdown sanitizer removed generated content.')
    return { kind: 'text', text }
  }

  return { kind: 'html', html }
}
