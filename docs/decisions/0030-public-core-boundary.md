# 0030 — Declare the public core boundary

- Status: accepted

## Decision

The shared core lives in `munimentai/muniment-core` under FSL, with its tests.
The desktop and the AI software factory build on it.
The desktop depends on the shared crates through git dependencies pinned to one release tag.
No desktop workspace crate vendors a shared crate or patches its source.

The desktop-only modules `auth`, `chat_grant` and `browser_control` live in the `muniment-desktop-integration` workspace crate.
The shared core never names them.
The desktop supplies their behavior through the core's port-owned values and traits.

## Shared

| Kind | Name | Why |
| --- | --- | --- |
| crate | muniment-core | It holds the local runtime logic and storage contracts. |
| crate | muniment-attach | It defines the reader and companion protocol without the shell. |
| crate | muniment-atomic-file | It publishes local files atomically. |
| crate | muniment-code-diff | It defines portable code-diff values and fixtures. |
| crate | muniment-pins | It pins Pi, its extension packages, and the Claude Code version. |
| crate | muniment-router | It selects provider accounts and routes model requests. |

## Desktop

| Kind | Name | Why |
| --- | --- | --- |
| crate | muniment-desktop | It owns the Tauri shell and entitlement projection UI. |
| crate | muniment-desktop-integration | It holds cloud sign-in, cloud chat grants, browser control, and the provider OAuth clients. |
| crate | muniment-runtime | It owns the user-level runtime service. |
| crate | muniment-cli | It gives the local runtime a shell-free client. |
| crate | muniment-acp | It adapts ACP agents to the runtime. |

The `browser-control/` extension and the remote-control design study also stay in this repository.
Neither is a workspace crate.
The reader interface stays small and hand-guarded through the attach protocol and journal projections.
The boundary does not authorize wider reader access or a protocol change.

## Host contracts

`muniment_core::account` holds the session, device, pairing, and entitlement values.
`auth` re-exports them and produces them for the runtime's `RunAttachBoundaries` calls.
`muniment_core::chat_launch::ChatGrant` is the launch value, and `ChatGrant::local` serves local mode.
`chat_grant` issues cloud grants and fetches cloud receipts.
The runtime's `PiLaunchBoundaries` renews grants, inspects the cloud session, and fetches receipts through it.
The shell's launch boundaries answer that the cloud is unavailable.
`browser_control` reads Linux process identities through `muniment_core::process_reader`.
The shell and the runtime register the Antigravity OAuth client with the router at startup.

## Check

`scripts/check-core-boundary.sh` reads both tables and rejects missing, duplicate, or unknown entries.
It requires every workspace crate in the Desktop table and no shared crate in the workspace.
It requires every shared crate to resolve from the `muniment-core` git repository at one tag.
It requires the three desktop-only modules in `muniment-desktop-integration`.
It checks every desktop crate except the shell with Cargo's all-target dependency tree, including build and test dependencies.
It rejects the shell and Tauri packages in those trees.
CI runs this check in the smoke job, including pushes to `main` and changes to this record.
`muniment-core` runs its own boundary check, which rejects any shared source that names a desktop-only module.
