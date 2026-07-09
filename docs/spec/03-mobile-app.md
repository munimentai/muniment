# Muniment Mobile App — Design Document

**Doc 3 of 5** · Tokens, laws, identity, components: see 01-design-system.md
**Scope:** mockup only — these screens exist to complete the brand story and de-risk future scope. Design them fully; do not engineer them. · **Status:** v1 · July 2026

---

## 1. Platform stance

iOS: bottom tab bar, large-title headers collapsing on scroll, system swipe-back, SF-weight icon rendering, native share sheet. Android: Material navigation bar, predictive back, system-standard ripples suppressed in favor of `faint` press states. **Android dynamic color is ignored** — territory doesn't recolor itself per phone; brand tokens win. Both respect OS light/dark with in-app override.

Type scale: 13 / 15 / 17 / 22 / 30. Hit targets ≥ 44px. Provenance and tool cards keep the mono register at 12px minimum.

## 2. Navigation

Bottom tabs, exactly three: **Threads · Projects · Inbox**. Projects tab hides entirely for users with no project membership (two-tab layout: Threads · Inbox). Profile chip lives in the Threads header, not a tab. No hamburger, no drawer.

## 3. The eight screens

### 3.1 Threads (home)
Large title "Threads". Search field. New-thread button (ink FAB on Android, header button on iOS). Rows: title (grotesque 15) + relative time (mono 12, muted). Swipe actions: share to project…, delete (typed-name confirm not required on mobile; delete is undoable for 8s via toast). Profile chip top-right → §3.8.

### 3.2 Thread
Same grammar as desktop, compressed: user bubbles right/`faint`; responses plain on `paper`, no avatar; pre-first-token = 17px thinking ring + `Routing · <label>` mono; streaming = signal underline + caret. **Tool cards arrive collapsed** (header only: status dot + verb + object); tap expands. Provenance line truncates to `route · cost`; tap opens the **receipt bottom sheet**: full route, matched rule, tokens, connections touched, lineage — mono, ledger layout. Message long-press: copy, fork from here, share…, read aloud, retry.

### 3.3 Composer + voice (the hero input)
Docked composer: input, attach, and a **large hold-to-talk control** (56px, ink ring at rest). Hold: control fills `faint`, 5-bar level meter animates above, verbatim transcript streams muted into the input. Release: signal underline flash while the on-device model polishes; transform chips (key points · formal · short · long) slide in for 6s. Slide-up while holding locks hands-free; tap to stop. All on-device; first use shows a one-time line: "Voice never leaves this phone."

### 3.4 Projects list
Rows: project name, member count (mono), unread-inbox dot (ink, not signal). Empty state for eligible-but-memberless users never occurs (tab hidden); this screen has no empty state by design.

### 3.5 Project detail
Header: project name + instructions preview (expandable). Segmented control: **Threads · Shared · Artifacts · Inbox**. (Connections is desktop/admin-only on mobile; a footer line notes "Connections are managed on desktop or by admins.") Threads = your threads in scope; new thread inherits scope.

### 3.6 Shared thread view
Read-only banner: "Shared by <name> · view only" with **Fork** as the primary header action. Includes the redaction pattern proven at small size: struck mono record `output withheld · connection not granted`, header visible, body withheld, surrounding prose intact. Fork lands the user in their own continuation with lineage noted in the first provenance receipt.

### 3.7 Inbox
Ledger rows: workflow · run time (mono) · status word · artifact count. Running rows: signal pulse on the status dot only. Tap → run detail: parameters, duration, result summary, artifacts (open in in-app viewer). Filter chips: all · mine · <project names>.

### 3.8 Entitlement peek (bottom sheet)
From the profile chip: appearance toggle, then **Your access** — groups as expandable mono sections listing models, connections, packages, capabilities. Read-only; footer "Access is set by your admins." Sign out at bottom.

## 4. Mobile-specific rules

- The thinking ring at ≤20px uses the solid reduction; fast-spin tier capped (design-system §5.3).
- Artifacts open in a full-screen viewer with version stepper; editing is desktop-only in v1 mockups (viewer shows "Edit on desktop").
- Notifications: workflow complete/failed, shared-thread mention. Deep-link into the exact row.
- Offline: same honesty as desktop — full-screen mono notice; no cached fake mode.
- No widgets, watch apps, or share extensions in mockup scope; leave hooks unstyled.

## 5. Mockup deliverables

Each of the eight screens in: iOS light, iOS dark, Android light, Android dark (32 frames), plus the voice interaction as a 6-frame sequence and the receipt bottom sheet in both themes. Use real content (the churn-analysis thread from the reference sheets), never lorem ipsum, never placeholder gray blocks.
