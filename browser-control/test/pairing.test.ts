import { describe, expect, it, vi } from 'vitest';

import { PAIRING_TOKEN_BYTES, PAIRING_TTL_MS, PairingSession } from '../src/index.js';

function session(overrides: Partial<ConstructorParameters<typeof PairingSession<string>>[0]> = {}) {
  let now = 1_000;
  let seed = 0;
  const verifyIdentity = vi.fn((expected: string, actual: string) => expected === actual);
  const pairing = new PairingSession({
    expectedIdentity: 'desktop-canonical-identity',
    verifyIdentity,
    now: () => now,
    randomBytes: bytes => bytes.fill(++seed),
    ...overrides,
  });
  return { pairing, verifyIdentity, setNow: (value: number) => { now = value; } };
}

describe('PairingSession', () => {
  it('authorizes exactly once with 256-bit opaque token material and OS-derived identity', async () => {
    let requestedBytes = 0;
    const { pairing, verifyIdentity } = session({
      randomBytes: bytes => { requestedBytes = bytes.byteLength; bytes.fill(7); },
    });
    const handoff = pairing.createHandoff();

    expect(requestedBytes).toBe(PAIRING_TOKEN_BYTES);
    expect(handoff).toEqual({ token: expect.any(String), expiresAt: 1_000 + PAIRING_TTL_MS });
    await expect(pairing.authorize(handoff.token, 'desktop-canonical-identity')).resolves.toEqual({ authorized: true });
    expect(verifyIdentity).toHaveBeenCalledWith('desktop-canonical-identity', 'desktop-canonical-identity');
    await expect(pairing.authorize(handoff.token, 'desktop-canonical-identity')).resolves.toMatchObject({ authorized: false, code: 'replayed' });
  });

  it('consumes a wrong token without consulting identity', async () => {
    const { pairing, verifyIdentity } = session();
    const handoff = pairing.createHandoff();
    await expect(pairing.authorize(`${handoff.token}x`, 'desktop-canonical-identity')).resolves.toMatchObject({ code: 'wrong-token' });
    await expect(pairing.authorize(handoff.token, 'desktop-canonical-identity')).resolves.toMatchObject({ code: 'replayed' });
    expect(verifyIdentity).not.toHaveBeenCalled();
  });

  it('rejects the exact expiry boundary without wall-clock sleeps', async () => {
    const { pairing, verifyIdentity, setNow } = session();
    const handoff = pairing.createHandoff();
    setNow(handoff.expiresAt);
    await expect(pairing.authorize(handoff.token, 'desktop-canonical-identity')).resolves.toMatchObject({ code: 'expired' });
    expect(verifyIdentity).not.toHaveBeenCalled();
  });

  it('fails closed when identity is rejected or the verifier throws', async () => {
    const rejected = session({ verifyIdentity: () => false }).pairing;
    const rejectedHandoff = rejected.createHandoff();
    await expect(rejected.authorize(rejectedHandoff.token, 'other-os-identity')).resolves.toMatchObject({ code: 'identity-rejected' });

    const errored = session({ verifyIdentity: () => { throw new Error('/private/browser/path'); } }).pairing;
    const erroredHandoff = errored.createHandoff();
    const result = await errored.authorize(erroredHandoff.token, 'other-os-identity');
    expect(result).toMatchObject({ code: 'verifier-error' });
    expect(JSON.stringify(result)).not.toContain(erroredHandoff.token);
    expect(JSON.stringify(result)).not.toContain('/private/browser/path');
  });

  it('rotates pairing material on replacement and successful reconnect', async () => {
    const { pairing } = session();
    const first = pairing.createHandoff();
    const replacement = pairing.replace('replacement-identity');
    expect(replacement.token).not.toBe(first.token);
    await expect(pairing.authorize(first.token, 'replacement-identity')).resolves.toMatchObject({ code: 'wrong-token' });

    const reconnect = pairing.replace('reconnect-identity');
    await expect(pairing.authorize(reconnect.token, 'reconnect-identity')).resolves.toEqual({ authorized: true });
    await expect(pairing.authorize(reconnect.token, 'reconnect-identity')).resolves.toMatchObject({ code: 'replayed' });
  });

  it('revokes and closes idempotently while invalidating prior material', async () => {
    const revoked = session().pairing;
    const revokedHandoff = revoked.createHandoff();
    revoked.revoke();
    revoked.revoke();
    await expect(revoked.authorize(revokedHandoff.token, 'desktop-canonical-identity')).resolves.toMatchObject({ code: 'revoked' });

    const closed = session().pairing;
    const closedHandoff = closed.createHandoff();
    closed.close();
    closed.close();
    await expect(closed.authorize(closedHandoff.token, 'desktop-canonical-identity')).resolves.toMatchObject({ code: 'closed' });
    expect(() => closed.createHandoff()).toThrow('Pairing session closed');
  });

  it('allows only one concurrent authorization through to an asynchronous verifier', async () => {
    let release!: (accepted: boolean) => void;
    const verification = new Promise<boolean>(resolve => { release = resolve; });
    const pairing = session({ verifyIdentity: () => verification }).pairing;
    const handoff = pairing.createHandoff();
    const first = pairing.authorize(handoff.token, 'desktop-canonical-identity');
    const concurrent = pairing.authorize(handoff.token, 'desktop-canonical-identity');
    release(true);
    await expect(first).resolves.toEqual({ authorized: true });
    await expect(concurrent).resolves.toMatchObject({ code: 'replayed' });
  });

  it('does not authorize a stale verifier after replacement', async () => {
    let release!: (accepted: boolean) => void;
    const verification = new Promise<boolean>(resolve => { release = resolve; });
    const pairing = session({ verifyIdentity: () => verification }).pairing;
    const handoff = pairing.createHandoff();
    const stale = pairing.authorize(handoff.token, 'desktop-canonical-identity');

    const replacement = pairing.replace('replacement-identity');
    release(true);
    await expect(stale).resolves.toMatchObject({ code: 'replayed' });
    await expect(pairing.authorize(replacement.token, 'replacement-identity')).resolves.toEqual({ authorized: true });
  });
});
