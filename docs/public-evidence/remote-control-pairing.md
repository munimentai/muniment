# Remote control pairing

A signed-in desktop with cloud features enabled can pair one phone to this
installation. Pairing is a Municloud account grant. Local attach approval for
CLI and editor programs is a separate surface.

The desktop pairs under contract revision `muniment.remote-control-pairing/2`.
For each code it creates a fresh X25519 agreement key pair and a 32-byte
pairing secret. It posts the revision and its agreement public key to
`/v1/remote-control/pairing/challenges` with the native access token. The
returned `qr` object must carry this installation's id, its installation
public key and the agreement key the desktop sent, or no code is shown. The QR
code encodes that object plus `pairing_secret` as compact UTF-8 JSON. The
pairing secret appears only in the QR code and never reaches the cloud.

The QR display ends at the server `expires_at` deadline, including the exact
boundary. While the pairing dialog is open, the desktop reads
`GET /v1/remote-control/pairing` at most once every two seconds. A new
challenge for the same installation waits ten seconds. An already paired
desktop cannot create a challenge.

When the phone authorizes, the pair carries both agreement keys and the
phone's proof, an HMAC-SHA256 under the pairing secret over the desktop and
phone ids, the challenge and both agreement keys. The desktop checks its own
agreement key and verifies the proof in constant time. On a mismatch it
revokes the pair and reports that the phone could not be verified. On success
it derives the desktop-to-phone and phone-to-desktop keys with HKDF-SHA256,
stores them in the operating-system keychain under the app's service, and
erases its agreement private key and the pairing secret. A code that expires
unused erases them too.

After authorization, Account settings shows the exact `mobile_device_id`. The
runtime stores `pair_id`, installation ids, peer public keys and the agreement
keys. Revoke sends `DELETE /v1/remote-control/pairing` with that `pair_id`,
deletes the pair's relay keys and counters, and clears the matching stored
peer identity. A later status with `pair: null` also clears them. A pair whose
keys this desktop does not hold, such as a revision 1 pair, shows **Pair
again**, which revokes the pair and shows a new code.

While a phone is paired and the account is signed in, the runtime keeps one
connection to the cloud relay at `/v1/events` and publishes each live run to
the phone, sealed under `muniment.relay-encryption/1` with AES-256-GCM. The
cloud sees only each event's session, run, sequence, phase, pending permission
gate and time. Assistant text, tool activity, run status and permission asks
travel encrypted. The phone can stop a run, steer it, queue a follow-up and
answer a permission gate. The desktop opens each command, rejects one whose
header changed, whose counter it already accepted within its 64-message
window, or that expired, and applies the rest through the same operations as
the desktop's own controls. A deny cancels a gate. An allow answers a
yes-or-no gate or applies proposed file changes. A command that does not
authenticate shows **Pair again** in Account settings.

Pairing grants remote control of desktop execution. The phone drives this
runtime. Local desktop use stays independent of account, mobile, and
company-record features.

Run the contract tests from the repository root:

```sh
cargo test --manifest-path src-tauri/integration/Cargo.toml --locked --lib remote_control_pairing
cargo test --manifest-path src-tauri/runtime/Cargo.toml --locked --lib pairing
```
