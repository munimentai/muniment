# Desktop authentication

The production Muniment handshake is installation-bound native auth. The Rust
core currently implements registration, coherent keychain persistence, and
the installation-proof authorization request. The older generic OIDC flow remains as tested groundwork, but it
is not the live production handshake (`/.well-known/openid-configuration`
returns 404 on the control plane).

## Native endpoint sequence

1. **Implemented:** `POST /v1/auth/native/devices` with client id
   `muniment-desktop`, client role `desktop`, platform `desktop`, and the
   canonical unpadded base64url encoding of a newly generated Ed25519 public
   key's raw 32 bytes. The response supplies a UUID device id, one-use
   registration token, device challenge, and a 600-second expiry.
2. **Implemented:** `POST /v1/auth/native/authorize` constructs and signs the
   canonical installation proof, validates the opaque HTTPS continuation, and
   persists the rotated device challenge.
3. **Implemented in pure core:** bind an ephemeral `127.0.0.1` callback,
   launch the validated opaque continuation in the system browser, validate
   callback state, and retain the authorization code with its PKCE verifier.
4. **Implemented in pure core:** `POST /v1/auth/native/token` exchanges the
   code with its PKCE, redirect, device, and signed installation-proof context,
   then atomically persists the token set and newly rotated device challenge.
5. **Follow-up:** `GET /v1/auth/native/session` inspects the authoritative
   session and consumes its entitlement snapshot; native revocation/device
   management follow the corresponding `/v1/auth/native/*` endpoints.

On first registration the raw private key, device id, registration token,
challenge, and absolute registration expiry are serialized as one record in
the platform keychain (`service: ai.muniment.desktop`, `user:
native-installation`). An existing record is returned without making a network
request. A failed keychain write publishes no partial installation locally.
Secret-bearing values use redacted `Debug` implementations and do not cross a
Tauri command boundary.

## Legacy generic OIDC groundwork

## Flow

Standards-compliant native-app flow (RFC 8252), driven entirely by OIDC
discovery. Sequence, as implemented in `src-tauri/core/src/auth/`:

1. `auth_sign_in` (Tauri command) starts the flow on a blocking worker.
2. Discovery: `GET {issuer}/.well-known/openid-configuration` resolves the
   authorization, token, and (optional) revocation endpoints. The document's
   `issuer` must match the configured one (RFC 8414 §3.3). Non-HTTPS issuers
   are rejected unless they are loopback (the in-test mock IdP).
3. A loopback listener binds `127.0.0.1` on an **ephemeral port**; the
   redirect URI is `http://127.0.0.1:{port}/callback`.
4. The **system browser** (RFC 8252 §7.2 — never an embedded webview) opens
   the authorization endpoint with `response_type=code`, the client id, the
   redirect URI, scopes, a fresh random `state`, and a PKCE S256
   `code_challenge` (RFC 7636).
5. The IdP redirects back to the loopback listener. The listener validates
   `state` (mismatch → the attempt is rejected, no token request is made),
   serves a plain "return to the app" page, and shuts down.
6. The code is exchanged at the token endpoint together with the PKCE
   `code_verifier`. Tokens are persisted via the `TokenStore` and the
   command returns a status payload (signed-in flag, subject, expiry).

`auth_status` answers from the stored tokens only — no network.
`auth_sign_out` clears the stored tokens, with best-effort RFC 7009
revocation first when discovery advertises a `revocation_endpoint`.

## Session freshness

`auth_ensure_fresh` checks stored expiry with a 60-second safety margin. Fresh
sessions and signed-out state require no network. Expired or nearly-expired
sessions are renewed with the refresh grant and the rotated tokens are saved.
If the provider rejects a dead refresh token (or none is stored), the local
session is cleared and signed-out status is returned. Discovery and network
failures are returned without clearing the stored session, so an unreachable
server never signs the user out. The core accepts the current time as an input
to keep this behavior deterministic in tests.

## Configuration surface

One place: constants at the top of `src-tauri/src/auth/mod.rs`, with env
overrides for development.

| Setting   | Default                                    | Override env        |
|-----------|--------------------------------------------|---------------------|
| Issuer    | `https://api.muniment.ai`                  | `MUNIMENT_ISSUER`   |
| Client id | `muniment-desktop`                         | `MUNIMENT_CLIENT_ID`|
| Scopes    | `openid profile email offline_access`      | —                   |

The default client id names the public desktop client registered on the
control plane. The environment override remains available for development
against another OIDC provider.

## Client registration (cloud side)

The control plane (better-auth OIDC provider on api.muniment.ai) registers
the desktop app as:

- **Public client** (native app). No client secret is issued or sent; the
  token endpoint auth method is `none`.
- **PKCE required**, method `S256`.
- **Grant types**: `authorization_code` and `refresh_token`.
- **Redirect URIs**: loopback `http://127.0.0.1:{port}/callback` with an
  **ephemeral port**. Per RFC 8252 §7.3 the server must compare loopback
  redirects ignoring the port. If better-auth only supports exact-match
  redirect URIs, it needs a loopback exception (or port-wildcard) for this
  client — that is the one non-default behavior this flow depends on.
- **Scopes**: `openid profile email offline_access` (`offline_access` so a
  refresh token is issued to keep desktop sessions short-lived-but-renewable,
  harness-spec §3.1).
- **Client id**: `muniment-desktop`.

The cloud registration and native-app PKCE support are live. The client flow
is also verified against a mock IdP in tests (below); wiring and exercising
the real handshake is the next client slice.

## Token storage

`muniment_core::auth::TokenStore` is the persistence boundary. Two
implementations:

- **`KeyringTokenStore`** (`src-tauri/src/auth/keyring_store.rs`): one
  keychain entry (`service: ai.muniment.desktop`, `user: oidc-tokens`)
  holding the token set as JSON. Backends via the `keyring` crate: macOS
  Keychain, Windows Credential Manager, Secret Service (D-Bus) on Linux.
  libdbus is vendored (compiled from source) so Linux builds need no extra
  system packages.
- **`InMemoryTokenStore`** (in `muniment-core`): tests and ephemeral use.

Invariants (harness-spec §3.1/§8): tokens are never logged and never touch
disk in plaintext. `TokenSet`'s `Debug` impl redacts token fields, error
types never embed token values, and the only persistence path is a
`TokenStore`. Only `AuthStatus` (signed-in flag, `sub`, expiry) crosses into
the webview.

## Tauri commands

| Command         | Returns                | Notes                             |
|-----------------|------------------------|-----------------------------------|
| `auth_sign_in`  | `AuthStatus` or error  | Runs the full browser flow; 5-min timeout; concurrent calls rejected |
| `auth_status`   | `AuthStatus`           | Local only, no network            |
| `auth_ensure_fresh` | `AuthStatus` or error | Refreshes at expiry or within 60 seconds |
| `auth_sign_out` | `AuthStatus`           | Best-effort revocation + clear    |

`AuthStatus` is `{ signed_in: bool, subject: string|null, expires_at: unix-seconds|null }`.

A temporary trigger row in `src/App.svelte` invokes these via
`window.__TAURI__.core.invoke` (`withGlobalTauri` is on); it disappears with
the real signed-in UI slice.

## Testing

`cargo test --manifest-path src-tauri/core/Cargo.toml` (what CI's smoke job
runs) covers: PKCE S256 correctness against the RFC 7636 appendix-B vector,
state-mismatch rejection (including that no token request is made),
full code exchange and refresh against an in-process mock IdP on
127.0.0.1, PKCE proof enforcement by the token endpoint, provider `error`
redirects, sign-out revocation, discovery issuer-mismatch rejection, and
token redaction in `Debug` output. No test touches api.muniment.ai.

The standalone `muniment-core` crate builds without TLS (tests speak plain
HTTP to loopback); the desktop crate enables the `tls` feature for real
HTTPS. Keychain access is deliberately not exercised in CI (the gate is a
build); verify it manually:

## Manual verification (`tauri dev`)

1. Point the app at the live control plane (or override both values for a
   local OIDC provider):
   `MUNIMENT_ISSUER=https://api.muniment.ai MUNIMENT_CLIENT_ID=muniment-desktop npm run tauri dev`
2. Click **sign in** — the system browser opens the IdP; complete the login.
3. The browser tab shows "Signed in — return to muniment"; the app's status
   line shows `signed in as <subject>`.
4. Restart the app; **status** still reports signed in (tokens came from the
   keychain — macOS Keychain Access / Windows Credential Manager / Secret
   Service under `ai.muniment.desktop`).
5. **sign out**; confirm the keychain entry is gone and **status** reports
   signed out.
6. Negative path: click **sign in** and cancel at the IdP — the app surfaces
   `access_denied` and stores nothing.

## Follow-ups (out of scope here)

- Native refresh and credential rotation.
- Authoritative native session inspection.
- Entitlement snapshot consumption.
- Migration or removal of credentials created by the existing generic-OIDC flow.
- Signed-in UI.
