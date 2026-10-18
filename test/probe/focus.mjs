import assert from 'node:assert/strict'

// Every control shows its focus ring under the keyboard. The walk tabs through
// the window and reads each focused element's outline: 2px solid, or, for the
// composer's text field, the composer frame's own edge.
export async function checkFocusRings(browser, baseUrl) {
  for (const fixture of ['history.html', 'in-flight.html', 'index.html?theme=moss-dark', 'index.html?theme=iris-contrast-light']) {
    const context = await browser.newContext({ viewport: { width: 1100, height: 720 }, reducedMotion: 'reduce' })
    const page = await context.newPage()
    try {
      await page.goto(`${baseUrl}/test/probe/${fixture}`)
      await page.waitForSelector('[data-probe-ready]')
      await page.evaluate(() => document.activeElement?.blur())
      const seen = new Set()
      for (let stop = 0; stop < 40; stop += 1) {
        await page.keyboard.press('Tab')
        const focus = await page.evaluate(() => {
          const element = document.activeElement
          if (!element || element === document.body) return null
          const style = getComputedStyle(element)
          const frame = element.matches('[data-focus-frame]') ? getComputedStyle(element.closest('.composer')) : null
          return {
            name: `${element.tagName.toLowerCase()} ${element.getAttribute('aria-label') ?? element.textContent.trim().slice(0, 30)}`,
            ring: style.outlineStyle === 'solid' && parseFloat(style.outlineWidth) >= 2,
            frame: !!frame && frame.outlineStyle === 'solid' && parseFloat(frame.outlineWidth) >= 1 && frame.borderTopColor === frame.outlineColor,
          }
        })
        if (!focus) continue
        seen.add(focus.name)
        assert.ok(focus.ring || focus.frame, `${fixture}: ${focus.name} shows no focus ring`)
      }
      assert.ok(seen.size >= 10, `${fixture}: the keyboard reached ${seen.size} controls`)
    } finally {
      await context.close()
    }
  }
  console.log('Focus ring checks passed.')
}
