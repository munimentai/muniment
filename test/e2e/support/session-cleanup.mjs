export async function waitForFixtureService(driver) {
  await driver.waitUntil(async () => {
    const status = await driver.execute(async () => (
      window.__TAURI__.core.invoke('attach_listener_status')
    ))
    return status.supervisor_running === true && status.connected === true
  }, {
    timeout: 60000,
    timeoutMsg: 'The desktop client did not connect before fixture cleanup.',
  })
}

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
