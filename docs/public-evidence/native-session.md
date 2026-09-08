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

## Cloud chat grants

The desktop posts `{"protocol":"muniment.desktop-access/1"}` to `/v1/chat/grants` with the native access token.
It accepts the `chat_grant` envelope and ignores compatible fields.
It rejects a grant for another installation.
The grant supplies the HTTPS gateway, short-lived key, and allowed model aliases.
The desktop selects the first allowed alias through a Pi provider extension.
The key stays out of the webview, settings, and run journal.
Before launch, the desktop renews a grant with at most 60 seconds of safe life.
Safe life excludes 30 seconds for clock skew.
The receipt request carries only `runId`.

Run the core contract tests from the repository root:

```sh
cargo test --manifest-path src-tauri/core/Cargo.toml --locked --test native_session
cargo test --manifest-path src-tauri/core/Cargo.toml --locked chat_grant
```
