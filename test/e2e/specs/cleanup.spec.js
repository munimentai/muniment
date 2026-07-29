describe('fixture cleanup', () => {
  it('best-effort revokes the persisted fixture session', async () => {
    const profile = await $('.profile-button')
    const signIn = await $('button=Sign in')
    const onboarding = await $('[data-testid="onboarding-home-path"]')
    let authState
    await browser.waitUntil(async () => {
      if (await profile.isDisplayed()) {
        authState = 'signed-in'
        return true
      }
      if (await signIn.isDisplayed()) {
        authState = 'signed-out'
        return true
      }
      if (await onboarding.isDisplayed()) {
        const signedIn = await browser.execute(async () => (
          await window.__TAURI__.core.invoke('auth_status')
        ).signed_in)
        if (!signedIn) {
          authState = 'onboarding'
          return true
        }
      }
      return false
    }, {
      timeout: 20000,
      timeoutMsg: 'authentication state did not settle before fixture cleanup',
    })
    if (authState === 'signed-in') {
      await profile.click()
      await (await $('button=Sign out')).click()
      await signIn.waitForDisplayed({ timeout: 20000 })
    }
  })
})
