import { revokeFixtureSession } from '../support/session-cleanup.mjs'

describe('fixture cleanup', () => {
  it('revokes the persisted fixture session and confirms sign-out', async () => {
    await revokeFixtureSession(browser)
  })
})
