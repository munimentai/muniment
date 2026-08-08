# The desktop's single mode and its cloud routing-surface name

Owner ruling, 2026-08-06. The desktop has one mode. That mode is not a chat
mode. The cloud embedding classifier classifies every desktop request.

## The mode name

The desktop's single mode is **the thread surface**.

SPEC law 13 and the ROADMAP Phase 3 entry use that name and no other name.
harness-spec §6.1 states the product rule behind it. One surface scales from a
short question to a full agentic run, and no mode switcher exists anywhere.
docs/spec/02-desktop-app.md §3 already heads the same surface "Thread surface".

## The classification rule

The desktop supplies no tier, no label, and no classification. The cloud
classifies every request at ingress with the pinned embedding classifier.
MUNICLOUD-968 ratified that path and merged it into muniment-cloud on
2026-08-07. It published `docs/public-evidence/prompt-classification.md` in
muniment-cloud. That file states the rule for outside readers: "Desktop, mobile,
and agent callers cannot supply routing classifications."

Two desktop requests reach the cloud directly. `fetch_grant` posts to
`/v1/desktop/chat/config` with no body. `fetch_receipt` posts one field,
`runId`. Every model call rides the grant the cloud issued.
`chat_coordinate.rs:203` passes only the scoped virtual key, the gateway URL,
and the optional pinned model into the sidecar environment.

## The routing-surface name, raised with MUNICLOUD

The cloud enumerates its routing surfaces as `RoutingSurface` in
`api/src/routing-policy-resolver.ts:5`. Its desktop value reads
`desktop_thread_chat`. That value names a mode the desktop does not have, so the
name contradicts the owner ruling above.

The enum is a cloud contract. The desktop therefore asks for the change and
renames nothing on its own.

| Field | Value |
| --- | --- |
| Cloud symbol | `RoutingSurface`, `api/src/routing-policy-resolver.ts:5` |
| Current value | `desktop_thread_chat` |
| Name the desktop asks for | `desktop_thread` |
| Raised by | MUNIDESK-969, 2026-08-07 |
| Ratifying MUNICLOUD ticket | none yet |
| Ratification status | OPEN |

`desktop_thread` is the smallest change that carries the ruling. It keeps the
`<surface>_<shape>` form that the sibling values `mobile_chat` and
`agent_gateway` follow. It keeps "thread", which both repositories already use
for this surface. It drops the one contested word.

The desktop never sends or stores this value. The cloud derives the surface from
the authenticated client role, through `chatSurface(context.clientRole)` in
`api/src/chat-routes.ts:192`. A rename therefore costs the desktop no
migration, and the cost sits with the cloud rows and evidence that record the old
string.

## Status, 2026-08-07

The classification half is ratified and built. MUNICLOUD-968 merged on
2026-08-07, and this repository states the matching rule in SPEC law 13.

The name half is open. No MUNICLOUD ticket has ruled on `desktop_thread` as of
2026-08-07. A search of muniment-cloud on that date found the value unchanged at
`api/src/routing-policy-resolver.ts:5` and found no merged or open pull request
that renames it.

Record the ratifying ticket in the table above once the ruling lands. File a
desktop follow-up if that ruling changes any name this repository holds.
