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

## Sign-in diagnostics

The runtime logs the `session.sign_in` RPC start, outcome, and elapsed time.
Each native cloud request logs its method, fixed path, HTTP status or transport error, and elapsed time.
Sign-in failures name the registration, authorization, token exchange, or proof stage without exposing credentials.
Authorization failures distinguish browser launch, callback timeout, state mismatch, and provider denial.

HTTP failures also log a recognized `error.code` and a validated `cf-ray` identifier beside the status.
Missing or unrecognized values appear as `unavailable`. The runtime never logs the response body.
The sign-in RPC returns `authorization_failed` with the authorization cause and HTTP error code instead of a persistence error.

The Windows runner records HTTPS clock samples in `clock.log` before the sign-in spec.
It corrects clock skew and requires a fresh sample within ten seconds, including sample uncertainty.
A failed clock check stops the sign-in spec and records the cause.

The response frame gets a fresh five-second deadline after sign-in returns, including failures.
A long browser wait does not consume that deadline.

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

An account without an allowed model receives `chat_not_entitled` from the cloud.
The failed reply shows that code and the cloud message without retrying the grant request.
The runtime logs the refusal code with the run id.
The desktop keeps its runtime connection after an authorization refusal.
The signed-in test reports a visible refusal instead of waiting for a receipt.

Run the core contract tests from the repository root:

```sh
cargo test --manifest-path src-tauri/core/Cargo.toml --locked --test native_session
cargo test --manifest-path src-tauri/core/Cargo.toml --locked --test native_sign_in
cargo test --manifest-path src-tauri/core/Cargo.toml --locked sign_in_returns_a_result
cargo test --manifest-path src-tauri/core/Cargo.toml --locked failure_diagnostics
cargo test --manifest-path src-tauri/core/Cargo.toml --locked chat_grant
```
