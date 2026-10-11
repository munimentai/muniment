// More providers ends the first-run list and opens the provider catalog in
// Settings. A runner with no model reaches Settings this way.
export async function openFirstRunModelSettings() {
  const more = await $('button=More providers')
  await more.waitForDisplayed()
  await more.waitForEnabled()
  await more.click()
}

// Back to threads sits at the top of the sidebar, so the sidebar opens first.
export async function closeSettings() {
  await expandSidebar()
  await (await $('button=Back to threads')).click()
}

export async function expandSidebar() {
  const toggle = await $('button[aria-controls="sidebar"]')
  await toggle.waitForDisplayed()
  if (await toggle.getAttribute('aria-expanded') === 'false') await toggle.click()
  await browser.waitUntil(async () => await toggle.getAttribute('aria-expanded') === 'true', {
    timeoutMsg: 'The sidebar did not expand.',
  })
}
