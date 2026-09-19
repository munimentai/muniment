export async function revokeFixtureSession(driver) {
  const result = await driver.execute(async () => {
    const invoke = window.__TAURI__.core.invoke
    const before = await invoke('auth_status')
    if (before.signed_in) await invoke('auth_sign_out')
    const after = await invoke('auth_status')
    return { signed_in: after.signed_in }
  })
  if (result?.signed_in !== false) {
    throw new Error('Fixture cleanup did not confirm sign-out.')
  }
}
