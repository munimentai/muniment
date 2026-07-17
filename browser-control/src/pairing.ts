export const PAIRING_TOKEN_BYTES = 32;
export const PAIRING_TTL_MS = 2 * 60 * 1000;

export type PairingFailureCode =
  | 'wrong-token'
  | 'expired'
  | 'replayed'
  | 'identity-rejected'
  | 'verifier-error'
  | 'revoked'
  | 'closed';

export type PairingResult =
  | { authorized: true }
  | { authorized: false; code: PairingFailureCode; message: string };

export interface PairingHandoff {
  readonly token: string;
  readonly expiresAt: number;
}

export interface PairingSessionOptions<Identity> {
  expectedIdentity: Identity;
  verifyIdentity(expectedIdentity: Identity, peerIdentity: Identity): boolean | Promise<boolean>;
  now?: () => number;
  randomBytes?: (bytes: Uint8Array) => void;
}

const FAILURE_MESSAGES: Record<PairingFailureCode, string> = {
  'wrong-token': 'Pairing token rejected',
  expired: 'Pairing token expired',
  replayed: 'Pairing attempt already consumed',
  'identity-rejected': 'Browser identity rejected',
  'verifier-error': 'Browser identity verification failed',
  revoked: 'Pairing session revoked',
  closed: 'Pairing session closed',
};

type TerminalState = 'consumed' | 'revoked' | 'closed';

/** Pure authorization boundary; socket ownership and process inspection are injected by callers. */
export class PairingSession<Identity> {
  readonly #now: () => number;
  readonly #randomBytes: (bytes: Uint8Array) => void;
  readonly #verifyIdentity: PairingSessionOptions<Identity>['verifyIdentity'];
  #expectedIdentity: Identity;
  #tokenBytes?: Uint8Array;
  #expiresAt = 0;
  #terminalState?: TerminalState;
  #generation = 0;

  constructor(options: PairingSessionOptions<Identity>) {
    this.#expectedIdentity = options.expectedIdentity;
    this.#verifyIdentity = options.verifyIdentity;
    this.#now = options.now ?? Date.now;
    this.#randomBytes = options.randomBytes ?? (bytes => crypto.getRandomValues(bytes));
  }

  createHandoff(): PairingHandoff {
    if (this.#terminalState === 'closed')
      throw new Error(FAILURE_MESSAGES.closed);
    this.#invalidateToken();
    const tokenBytes = new Uint8Array(PAIRING_TOKEN_BYTES);
    this.#randomBytes(tokenBytes);
    this.#tokenBytes = tokenBytes;
    this.#expiresAt = this.#now() + PAIRING_TTL_MS;
    this.#terminalState = undefined;
    this.#generation++;
    return { token: encodeToken(tokenBytes), expiresAt: this.#expiresAt };
  }

  replace(expectedIdentity: Identity): PairingHandoff {
    this.#expectedIdentity = expectedIdentity;
    return this.createHandoff();
  }

  async authorize(token: string, peerIdentity: Identity): Promise<PairingResult> {
    if (this.#terminalState)
      return failure(this.#terminalState === 'consumed' ? 'replayed' : this.#terminalState);
    const expectedToken = this.#tokenBytes;
    if (!expectedToken)
      return failure('replayed');

    // Consume before awaiting the verifier so concurrent attempts cannot both succeed.
    const generation = this.#generation;
    this.#terminalState = 'consumed';
    this.#tokenBytes = undefined;
    const expired = this.#now() >= this.#expiresAt;
    const matches = tokenMatches(token, expectedToken);
    expectedToken.fill(0);
    if (expired)
      return failure('expired');
    if (!matches)
      return failure('wrong-token');

    try {
      const accepted = await this.#verifyIdentity(this.#expectedIdentity, peerIdentity);
      if (generation !== this.#generation)
        return failure('replayed');
      return accepted ? { authorized: true } : failure('identity-rejected');
    } catch {
      if (generation !== this.#generation)
        return failure('replayed');
      return failure('verifier-error');
    }
  }

  revoke(): void {
    if (this.#terminalState === 'closed')
      return;
    this.#invalidateToken();
    this.#terminalState = 'revoked';
    this.#generation++;
  }

  close(): void {
    if (this.#terminalState === 'closed')
      return;
    this.#invalidateToken();
    this.#terminalState = 'closed';
    this.#generation++;
  }

  #invalidateToken(): void {
    this.#tokenBytes?.fill(0);
    this.#tokenBytes = undefined;
    this.#expiresAt = 0;
  }
}

function failure(code: PairingFailureCode): PairingResult {
  return { authorized: false, code, message: FAILURE_MESSAGES[code] };
}

function encodeToken(bytes: Uint8Array): string {
  let binary = '';
  for (const byte of bytes)
    binary += String.fromCharCode(byte);
  return btoa(binary).replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/, '');
}

function tokenMatches(token: string, expected: Uint8Array): boolean {
  const actual = new TextEncoder().encode(token);
  const encoded = new TextEncoder().encode(encodeToken(expected));
  let difference = actual.byteLength ^ encoded.byteLength;
  for (let index = 0; index < Math.max(actual.byteLength, encoded.byteLength); index++)
    difference |= (actual[index] ?? 0) ^ (encoded[index] ?? 0);
  return difference === 0;
}
