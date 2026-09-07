# Native session inspection

The desktop reads `/v1/auth/native/session` for current cloud session details.
It accepts compatible response fields without passing unknown fields to the webview.
It checks the session device and client role against the installation.
It rejects conflicting session, user, organization, and snapshot identities.

The profile shows the user email, organization name, and membership role.
The access panel shows the payload entitlement version, capabilities, and grants.
Each grant shows its effect, action, resource, principal, and expiration.
The panel preserves allow and deny records rather than computing effective access.
Entitlement snapshots are display hints. The server enforces cloud use.

Run the core contract tests from the repository root:

```sh
cargo test --manifest-path src-tauri/core/Cargo.toml --locked --test native_session
```
