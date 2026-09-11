export async function openFirstRunModelSettings() {
  await (await $('button=Send')).click()
  const model = await $('[data-testid="onboarding-model"]')
  await model.waitForDisplayed()
  const panel = await $('#onboarding-model-panel')
  await panel.waitForDisplayed()
  expect(await model.getAttribute('aria-expanded')).toBe('true')
  const settings = await panel.$('button=Open model settings')
  await settings.waitForDisplayed()
  expect(await (await panel.$('p=No free hosted model exists at the no-account tier.')).isDisplayed()).toBe(true)
  await settings.waitForEnabled()
  await settings.click()
}

export async function expandSidebar() {
  const toggle = await $('button[aria-controls="sidebar"]')
  await toggle.waitForDisplayed()
  if (await toggle.getAttribute('aria-expanded') === 'false') await toggle.click()
  await browser.waitUntil(async () => await toggle.getAttribute('aria-expanded') === 'true', {
    timeoutMsg: 'The sidebar did not expand.',
  })
}
