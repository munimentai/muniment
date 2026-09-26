# Remote control pairing

A signed-in desktop with cloud features enabled can pair one phone to this
installation. Pairing is a Municloud account grant. Local attach approval for
CLI and editor programs is a separate surface.

The desktop posts `{"contract_version":"muniment.remote-control-pairing/1"}` to
`/v1/remote-control/pairing/challenges` with the native access token. The QR
code encodes the exact returned `qr` object as compact UTF-8 JSON. That object
holds the contract revision, desktop installation id, desktop public key, and
challenge. Those four fields are the whole QR payload.

The QR display ends at the server `expires_at` deadline, including the exact
boundary. While the pairing dialog is open, the desktop reads
`GET /v1/remote-control/pairing` at most once every two seconds. A new
challenge for the same installation waits ten seconds. An already paired
desktop cannot create a challenge.

After authorization, Account settings shows the exact `mobile_device_id`. The
runtime stores `pair_id`, installation ids, and peer public keys. Revoke sends
`DELETE /v1/remote-control/pairing` with that `pair_id` and then clears the
matching stored peer identity. A later status with `pair: null` also clears it.

Pairing grants remote control of desktop execution. The phone drives this
runtime. Local desktop use stays independent of account, mobile, and
company-record features.

Run the contract tests from the repository root:

```sh
cargo test --manifest-path src-tauri/core/Cargo.toml --locked --test remote_control_pairing
cargo test --manifest-path src-tauri/runtime/Cargo.toml --locked --test pairing
```
