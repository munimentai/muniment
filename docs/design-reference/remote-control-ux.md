# Remote Control desktop UX reference

**Status:** Design note, measured 2026-07-29. Every proposal in this note is
pending owner mockup confirmation. Owner mockups remain ground truth.

## Boundary

This repository has no Remote Control code today. A search of `src/`,
`src-tauri/`, and `test/` for `remote_control`, `remoteControl`, and
`remote-control` returns no matches.

[Harness specification §14](../spec/harness-spec.md#14-remote-control-owner-decision-2026-07-17)
and [ADR 0012](../decisions/0012-user-level-runtime-service.md) ratify the
surface but do not build it. The specification requires a desktop session
action, a persistent attached banner, a disconnect control, and an expiring
URL plus QR handoff. ADR 0012 assigns the single outbound relay leg to the
runtime service. The shipped MV3 loopback relay controls a browser and is not
this surface. Journal-backed relay publication also remains deferred in
[ADR 0002](../decisions/0002-event-sourced-run-journal.md).

The requested polish has no implementation to change. This note sets a
standard for the future surface.

## Reference study

### T3 Code

| Claim | Source status |
|---|---|
| The server issues a one-time owner pairing token. A remote device exchanges it for a session. | **Primary-source verified:** [remote access guide](https://github.com/pingdotgg/t3code/blob/main/docs/user/remote-access.md#how-pairing-works) |
| The desktop makes a pairing link. The CLI prints a URL and QR code. | **Primary-source verified:** [remote access guide](https://github.com/pingdotgg/t3code/blob/main/docs/user/remote-access.md#enabling-network-access) |
| The desktop lists reachable endpoints and persists the default endpoint type. | **Primary-source verified:** [remote access guide](https://github.com/pingdotgg/t3code/blob/main/docs/user/remote-access.md#option-1-desktop-app) |
| The local server owns sessions, pairing, files, and provider processes. | **Third-party unverified:** [unofficial architecture guide](https://t3codedocs.com/docs/architecture/) |
| Paired environments show online, checking, and error states. | **Third-party unverified:** the [unofficial guide](https://t3codedocs.com/docs/architecture/) provides no primary product evidence for these component states. |
| Reconnection, latency compensation, and optimistic rendering create the admired polish. | **Third-party unverified:** the [unofficial architecture guide](https://t3codedocs.com/docs/architecture/) does not document these behaviors. |

T3 Code informs the compact URL and QR handoff. Muniment does not copy its
network model, code, Electron choice, or component behavior.

### Shipped comparables

| Product | Shipped behavior | Muniment takes |
|---|---|---|
| [Claude Code Remote Control](https://code.claude.com/docs/en/remote-control) | The CLI prints a session URL and toggleable QR code. A footer indicator shows the active state. The local process executes work. An outage ends the session after about ten minutes. An online machine has a green status dot. | Keep the handoff close to the session. Show a bounded reconnect countdown. State that work remains local. Do not take its green status color. |
| [VS Code Live Share](https://learn.microsoft.com/en-us/visualstudio/liveshare/use/share-project-join-session-visual-studio-code#session-states-and-limitations) | A persistent, low-chrome status-bar item names the session state and opens its controls. | Keep steady state persistent and compact. Make the state control open its details. |
| [Google Cast](https://developers.google.com/cast/docs/design_checklist) | The connected state names the receiver beside a Stop Casting control without alarm chrome. | Name the controlling device when available. Pair it with a direct disconnect control. |

[web.dev offline guidance](https://web.dev/articles/offline-ux-design-guidelines)
also says network state should not block content. It calls for plain language
and other cues alongside color. Muniment therefore keeps reconnecting
non-modal and never relies on color.

## Pending desktop proposal

All details below remain pending owner mockup confirmation.

The session menu gets one action: **Start Remote Control**. It opens a radius
`10` modal on `surface` with `shadow-overlay`. The modal shows this exact copy:

- Heading: **Control this session from another device**
- Body: **Scan the code or open the link on your signed-in device.**
- Target record: **Session:** `{session title}`
- Authority record: **Organization:** `{organization name}`
- Expiry record: **Link expires in** `{mm:ss}`
- Actions: **Copy link** and **Done**

The session, organization, and expiry records use Commit Mono and `muted`.
The URL appears in full beside the QR code. Its visible session title,
organization, and countdown make the target and lifetime legible.

The handoff remains short-lived, single-use, session-bound, and user-bound.
Possession never grants authority. These rules restate §14.3 and do not change
the relay contract.

While attached, a persistent banner sits above the composer. It uses `surface`,
a `border` hairline, radius `6`, and `ink` text. Device names use Commit Mono.
The banner has no icon, motion, shadow, `signal`, alarm chrome, or `oxide`.
Its focus outline uses `ink`.

| Runtime state | Exact copy | Control |
|---|---|---|
| Connected | **Remote controlled from** `{device name}` | **Disconnect** |
| Reconnecting | **Remote control interrupted. Reconnecting for** `{mm:ss}` | **Disconnect** |
| Disconnected | **Remote control disconnected. The local session continues.** | No action |

Connected and reconnecting use the persistent banner. Reconnecting keeps the
last verified device name in its details, not its headline. The countdown
uses tabular Commit Mono numerals and the runtime-authoritative deadline.

Disconnected removes the banner and uses the existing toast primitive:
bottom-center, `surface`, `border` hairline, radius `6`, and `ink` text. It
auto-dismisses after five seconds. It uses no action, `signal`, or `oxide`.
A stale client never restores the banner. The user must start Remote Control
again to make a new handoff.

The tray ring retains its existing §9 meaning. It thinks only for active work,
not for a Remote Control connection or reconnect attempt. The full-surface
server-unreachable notice and entitlement toast keep their exact §10 copy.

## Deliberate non-decisions

This note does not decide or change:

1. The frozen relay contract, transport, message schema, reconnect authority,
   or runtime state machine.
2. The mobile screens, navigation, controls, or session-list presentation.
3. The entitlement copy, eligibility rules, owner kill-switch copy, or
   entitlement toast.

Owner mockups must confirm every proposal before implementation.
