import { afterEach, describe, expect, it } from 'vitest'
import './tokens.css'
import './code-diff.css'

function mountDiff() {
  document.body.innerHTML = `
    <section class="code-diff">
      <div class="d2h-wrapper">
        <div class="d2h-file-wrapper">
          <div class="d2h-file-header"><span class="d2h-file-name">src/file.js</span></div>
          <div class="d2h-file-diff">
            <div class="d2h-files-diff">
              <div class="d2h-file-side-diff">
                <div class="d2h-code-side-line d2h-del"><del>old</del></div>
                <div class="d2h-code-side-line d2h-del d2h-change">changed</div>
              </div>
              <div class="d2h-file-side-diff">
                <div class="d2h-code-side-line d2h-ins"><ins>new</ins></div>
                <div class="d2h-code-side-line d2h-ins d2h-change">changed</div>
              </div>
            </div>
            <div class="d2h-info">one unchanged line</div>
            <div class="d2h-emptyplaceholder"></div>
          </div>
        </div>
      </div>
    </section>`
}

function color(selector, property = 'backgroundColor') {
  return getComputedStyle(document.querySelector(selector))[property]
}

function computedBackground(value) {
  const probe = document.createElement('div')
  probe.style.backgroundColor = value
  document.body.append(probe)
  return getComputedStyle(probe).backgroundColor
}

function token(name) {
  return computedBackground(`var(${name})`)
}

describe('code diff browser styles', () => {
  afterEach(() => {
    document.documentElement.removeAttribute('data-theme')
    document.body.replaceChildren()
  })

  for (const theme of ['light', 'dark']) {
    it(`resolves visible colors in the explicit ${theme} theme`, () => {
      document.documentElement.dataset.theme = theme
      mountDiff()

      expect(color('.d2h-file-wrapper')).toBe(token('--surface'))
      expect(color('.d2h-file-header')).toBe(token('--faint'))
      expect(color('.d2h-file-name', 'color')).toBe(token('--ink'))
      expect(color('.d2h-info')).toBe(token('--faint'))
      expect(color('.d2h-emptyplaceholder')).toBe(token('--faint'))
      expect(color('.d2h-ins')).toBe(token('--signal-soft'))
      expect(color('.d2h-del')).toBe(computedBackground('color-mix(in srgb, var(--oxide) 12%, var(--surface))'))
      expect(color('.d2h-del.d2h-change')).toBe(computedBackground('color-mix(in srgb, var(--oxide) 24%, transparent)'))
      expect(color('.d2h-ins.d2h-change')).toBe(computedBackground('color-mix(in srgb, var(--signal) 24%, transparent)'))
    })
  }

  it('stacks side-by-side panels in the narrow viewport', () => {
    mountDiff()
    const panels = [...document.querySelectorAll('.d2h-file-side-diff')]
    const first = panels[0].getBoundingClientRect()
    const second = panels[1].getBoundingClientRect()

    expect(document.documentElement.clientWidth).toBeLessThanOrEqual(720)
    expect(getComputedStyle(document.querySelector('.d2h-files-diff')).display).toBe('block')
    expect(first.width).toBeGreaterThan(0)
    expect(second.top).toBeGreaterThanOrEqual(first.bottom)
    expect(second.width).toBeCloseTo(first.width, 0)
  })
})
