import assert from 'node:assert/strict'

// Every control shows its focus ring under the keyboard. The walk tabs through
// the window and reads each focused element's outline: 1px solid in the
// accent, or, for a text field whose frame shows focus, that frame: the
// composer's own edge.
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
          const host = element.matches('[data-focus-frame]') ? element.closest('.composer, .settings-search, .palette, .launcher') : null
          const frame = host?.matches('.composer') ? getComputedStyle(host) : null
          return {
            name: `${element.tagName.toLowerCase()} ${element.getAttribute('aria-label') ?? element.textContent.trim().slice(0, 30)}`,
            ring: style.outlineStyle === 'solid' && parseFloat(style.outlineWidth) >= 1,
            frame: !!host && (!frame || (frame.outlineStyle === 'solid' && parseFloat(frame.outlineWidth) >= 1 && frame.borderTopColor === frame.outlineColor)),
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
  await checkPointerFocus(browser, baseUrl)
  console.log('Focus ring checks passed.')
}

// A pointer press never draws a ring: the tools menu focuses its first item
// on a click. The command palette's field draws none as it opens either,
// since the palette shows focus itself.
async function checkPointerFocus(browser, baseUrl) {
  const context = await browser.newContext({ viewport: { width: 1100, height: 720 }, reducedMotion: 'reduce' })
  const page = await context.newPage()
  const ring = () => page.evaluate(() => {
    const style = getComputedStyle(document.activeElement)
    return style.outlineStyle !== 'none' && parseFloat(style.outlineWidth) > 0 ? document.activeElement.textContent.trim() || document.activeElement.getAttribute('aria-label') : ''
  })
  try {
    await page.goto(`${baseUrl}/test/probe/history.html?theme=moss-dark`)
    await page.waitForSelector('[data-probe-ready]')
    await page.click('button[aria-label="Workspace tools"]')
    await page.waitForSelector('[role="menuitem"]:focus')
    assert.equal(await ring(), '', 'the tools menu draws a ring on a clicked item')
    await page.keyboard.press('Escape')
    // The palette opens on its shortcut, the one the sidebar's chip names.
    await page.keyboard.press(await page.getAttribute('button[aria-label="Open the command palette"]', 'aria-keyshortcuts'))
    await page.waitForSelector('[role="combobox"]:focus')
    assert.equal(await ring(), '', 'the command palette draws a ring on its field')
  } finally {
    await context.close()
  }
}
