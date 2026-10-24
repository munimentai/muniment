export const SETTINGS_DIALOG = '[role="dialog"][aria-label="Settings"]'

// More providers ends the first-run list and opens the provider catalog in
// Settings. A runner with no model reaches Settings this way. The helper clicks
// once and waits for the real outcome: the catalog in the Settings dialog, or
// the first-run error the click produced. A failure names that error and the
// startup state, so a failed transition is never hidden by a second click.
export async function openFirstRunModelSettings({ timeout = 120000 } = {}) {
  const more = await $('button=More providers')
  await more.waitForDisplayed()
  await more.waitForEnabled()
  await more.click()
  const settings = await $(SETTINGS_DIALOG)
  let failure
  try {
    await browser.waitUntil(async () => {
      const alert = await $('section.onboarding p.error[role="alert"]')
      if (await alert.isDisplayed()) {
        failure = (await alert.getText()).trim()
        if (failure) return true
      }
      if (!(await settings.isDisplayed())) return false
      return await (await settings.$('h4=Connect account')).isDisplayed()
    }, { timeout, timeoutMsg: 'More providers did not open the provider catalog in Settings.' })
  } catch (error) {
    throw new Error(`${error.message} ${await firstRunState()}`)
  }
  if (failure) throw new Error(`More providers failed: ${failure} ${await firstRunState()}`)
  return settings
}

// The startup state a failed first-run transition reports: whether Home is
// configured, local mode, the desktop client connection and which screens
// render. Paths stay out of the message.
export async function firstRunState() {
  let state = 'unavailable'
  try {
    state = JSON.stringify(await browser.execute(async () => {
      const ask = (command) => Promise.race([
        window.__TAURI__.core.invoke(command),
        new Promise((_, reject) => setTimeout(() => reject(new Error(`${command} timed out`)), 5000)),
      ]).then((result) => ({ result }), (error) => ({ error: String(error?.message ?? error) }))
      const home = await ask('home_status')
      return {
        home: home.result ? { configured: home.result.configured, path: !!home.result.homePath } : home,
        localMode: await ask('local_mode_status'),
        desktopClient: await ask('attach_listener_status'),
        settingsDialog: !!document.querySelector('[role="dialog"][aria-label="Settings"]'),
        firstRun: !!document.querySelector('section.onboarding'),
      }
    })) ?? 'unavailable'
  } catch {}
  return `First-run state: ${state}`
}

// The Settings control is the gear in the sidebar's foot.
export async function openSettings() {
  await expandSidebar()
  const settings = await $(SETTINGS_DIALOG)
  if (!(await settings.isDisplayed())) {
    const control = await $('button.dock-button[aria-label="Settings"]')
    await control.waitForDisplayed()
    await control.click()
  }
  await settings.waitForDisplayed({ timeoutMsg: 'The Settings dialog did not open.' })
  return settings
}

// Open Settings lists its pages in the sidebar.
export async function openSettingsPage(name) {
  const settings = await openSettings()
  const sections = await $('nav[aria-label="Settings sections"]')
  await sections.waitForDisplayed()
  await (await sections.$(`button=${name}`)).click()
  return settings
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
