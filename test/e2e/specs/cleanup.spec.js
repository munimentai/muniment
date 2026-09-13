describe('fixture cleanup', () => {
  it('best-effort revokes the persisted fixture session', async () => {
    const profile = await $('.profile-button')
    const signIn = await $('button=Sign in')
    // The signed-out Local mode shell offers the cloud sign-in from its panel and holds no session.
    const localModeSignIn = await $('button=Sign in for cloud features')
    const onboarding = await $('[data-testid="onboarding-home-path"]')
    let authState
    await browser.waitUntil(async () => {
      if (await profile.isDisplayed()) {
        authState = 'signed-in'
        return true
      }
      if (await signIn.isDisplayed() || await localModeSignIn.isDisplayed()) {
        authState = 'signed-out'
        return true
      }
      if (await onboarding.isDisplayed()) {
        const signedIn = await browser.executeAsync((done) => {
          window.__TAURI__.core.invoke('auth_status')
            .then((status) => done(status.signed_in))
            .catch(() => done(true))
        })
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
