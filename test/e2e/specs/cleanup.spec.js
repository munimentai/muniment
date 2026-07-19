describe('fixture cleanup', () => {
  it('best-effort revokes the persisted fixture session', async () => {
    const profile = await $('.profile-button')
    if (await profile.isExisting()) {
      await profile.click()
      await (await $('button=Sign out')).click()
    }
    await (await $('button=Sign in')).waitForDisplayed({ timeout: 20000 })
  })
})
