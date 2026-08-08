# Muniment Desktop App Design Document

**Doc 2 of 5** · Tokens, laws, identity, components: see 01-design-system.md
**Runtime:** Tauri v2 · **Status:** v1 for handoff · July 2026

---

## 1. Platform chrome (the platform has its say)

- **macOS:** traffic lights top-left inside our titlebar row; native menu bar (File/Edit/View/Thread/Help); `⌘` shortcuts; solid `surface` titlebar (no vibrancy); native notifications, file dialogs, context menus.
- **Windows:** caption buttons top-right; `Ctrl` shortcuts; no Mica/acrylic; app menu behind a titlebar button.
- **Linux:** client-side decorations matching our titlebar; `Ctrl` shortcuts.
- Brand tokens identical everywhere; only chrome placement and modifier keys vary. Window min-size 960×640; state (size, position, sidebar) restored per display arrangement.

## 2. Layout

Three regions: **sidebar** (left, collapsible), **thread** (center, fluid), **artifact rail** (right, closed by default). Thread content max-width 760px centered when the rail is closed; reflows when open (rail 380–560px, draggable). Titlebar row carries session name (editable inline), artifact toggle `⌘J`, palette hint `⌘K`.

### 2.1 Sidebar

Expanded by default (260px), state remembered; collapse (`⌘\`) animates 180ms to a 52px icon rail with tooltips.

Top→bottom: **New thread** (`⌘N`) · **Search** (`⌘F` global) · **Threads** (recents; title + relative time, no content previews; context menu: rename, share to project…, delete) · **Projects** (section renders only if the user belongs to ≥1 project; solo users never see the concept) · **Inbox** (only if the user owns workflows or belongs to a project) · **Profile block**.

**Profile block:** no avatar. `mikey · dnsfilter · owner`. The name uses grotesque. The organization and role use muted mono. Click → popover: appearance (System/Light/Dark), **Your access** (entitlement peek: groups list; each expands to models, connections, packages, capabilities granted, read-only, mono; footer "Access is set by your admins"), keyboard shortcuts, sign out.

### 2.2 Command palette (`⌘K`)

Supplements navigation, never replaces it. Sections: threads, projects, artifacts, actions ("New thread in <project>", "Open inbox", "Toggle appearance", "Pin model…" if entitled). Fuzzy match; recent-first; mono for record-type results.

## 3. Thread surface

One mode. No mode switcher exists anywhere, including menus and settings.

### 3.1 Message grammar

- **User message:** right-aligned, `faint` bubble, radius 10, max-width 78%. Attachments as chips inside the bubble.
- **Response:** left-aligned plain text on `paper`, with no bubble or avatar. The model writes onto the org's page.
- **Pre-first-token:** the milled ring (thinking state, 17px) + mono status: `Routing · analysis/high`. Ring and streaming underline never animate at once; the ring resolves when the first token lands.
- **Streaming:** active line carries the 2px signal underline + signal caret. Color leaves at completion.
- **Tool activity:** inline tool cards per design-system §6. Sequential tools stack; parallel tools render as a grouped card with per-tool status rows.
- **Provenance line:** under every response. Click expands the receipt inline: classifier label, matched policy rule (mono, e.g. `rule 7: analysis/high → glm-5.2, fallback opus-4.8`), token counts, connections touched, fork lineage if any. This element is never hidden by any setting.
- **Model pin (entitled users only):** mono chip `auto ▾` right of the provenance area. Pinned state renders in ink: `pinned → glm-5.2` (a user decision is not computation). Pin persists per thread. Users without `router.override` never see the chip.

### 3.2 Message actions

Hover (or focus) reveals a quiet action row: copy, fork from here, share…, retry (responses only). Retry re-routes; the new provenance line shows the change. Edits to user messages create a visible branch marker, not silent history rewriting. This preserves ledger honesty.

## 4. Composer

`surface`, radius 10, focus stroke `muted`. Rows: input (auto-grow to 10 lines, then scroll) · action row: **voice** · **attach** · **Send** (ink button; `⏎` sends, `⇧⏎` newline). Hint line (muted 11px): default "Routing is automatic. Every reply carries its receipt."; contextual variants (project scope, pinned model) replace it.

**Attachments:** drag-drop anywhere on the thread; paste images; picker via attach. Chips show name + size (+48px thumb for images). Per-model caps read from the model registry; oversize = inline mono error with the cap stated. docx/xlsx/csv extract client-side with a mono "extracted N pages/rows" note on the chip.

## 5. Voice

Global hold-to-talk `⌥Space` (macOS) / `Alt+Space` alternative binding on Windows if the system menu conflicts (settle in build; expose rebinding in settings). Click-and-hold the voice button does the same; double-tap toggles hands-free until `Esc`.

1. **Capturing:** hint line becomes a 5-bar level meter (muted bars). Verbatim transcript streams into the input in muted text.
2. **Polishing (on release):** transcript flashes the signal underline while the local model strips fillers and applies self-corrections. This is the only signal inside the composer because computation occurs. The transcript then settles to ink. Transform chips (key points · formal · short · long) appear for 6s; `⌥1–4` applies.
3. `Esc` cancels and restores prior input. Read-aloud: response context menu → "Read aloud" (Kokoro); a quiet stop control appears in the titlebar row while speaking.

All voice processing is on-device; settings state this plainly.

## 6. Artifact rail

`⌘J` or clicking an artifact reference opens the rail. Contents: rendered artifact (md/html/code/diagram), version stepper (mono `v3 · 2h ago`), actions: copy, download, save to project… (entitled projects only), open in window. Multiple artifacts in a thread stack as rail tabs. Artifacts referenced from a project open read-only with an "Edit a copy" action if the user lacks `edit`.

## 7. Projects (team surface)

A project = shared workspace root + files + artifacts + connections + instructions. Opening one swaps the titlebar to project tabs: **Threads · Shared · Artifacts · Inbox · Connections**.

- **Threads:** your threads inside the project scope. New threads here inherit project instructions + workspace root + connection set.
- **Shared:** live read-only views of threads members shared in. **Redaction rule:** tool-output from a connection the viewer lacks renders as a struck mono record: `output withheld · connection not granted`. The header stays visible, the body stays hidden, and surrounding reasoning text remains. You can see that something happened and why you can't see it.
- **Fork** (primary action on shared threads): continues as the viewer's own thread under their own key/entitlements. Lineage recorded in provenance.
- **Artifacts:** the project library; group-permission chips (mono) on each; publish flow if entitled.
- **Inbox:** workflow results as ledger rows (workflow · run time · status · artifacts). Running rows carry the signal pulse. Personal inbox aggregates across projects + personal workflows.
- **Connections:** read-only list of the project's MCP connections and skills with grant source (`via group:data-team`). Nothing to configure for non-admins; admins get a "Manage in admin" deep link.

## 8. Sandboxing UX (permission gates)

Default mode: workspace-scoped. First out-of-scope file/command triggers an inline ask card (mono): the exact path/command, Allow once / Allow for this thread / Deny. Owner policy can force always-ask per group; the card then says so ("Your org requires per-action approval"). Full-auto (entitled + isolated platforms only) is a per-thread toggle in the thread menu with a plain-language consequence line; on Windows it offers WSL2 or server-side run instead, never fake isolation.

## 9. System presence

- **Tray/menubar icon:** ring at rest; thinking state while any workflow or backgrounded thread runs. Menu: open, new thread, inbox (badge count), pause workflows (entitled).
- **Notifications (native):** workflow completed/failed, shared-thread mention, inbox delivery. Quiet hours follow OS focus modes.
- **Dock/taskbar badge:** inbox unread count.

## 10. States

- **First run:** empty thread with one line: "Ask anything. Your org's routing decides which model answers." Composer focused. No tour.
- **Server unreachable:** full-surface mono notice: cause, retry countdown, "Copy diagnostics". No fake offline mode.
- **Entitlement change:** toast "Your access changed. Some models or connections may differ."
- **Model/provider failure:** inline mono record: `provider timeout · retried on fallback` with the provenance line showing the actual route taken.
- **Empty search:** "No threads match. `⌘N` starts one."

## 11. Settings (deliberately small)

Appearance · Voice (input device, hotkey rebind, read-aloud voice, on-device statement) · Notifications · Keyboard shortcuts · About (version, licenses page crediting the open stack). Everything else lives in the admin app; a footer line says so with a deep link for admins/owners.

## 12. Keyboard map (defaults)

`⌘N` new thread · `⌘K` palette · `⌘F` search · `⌘J` artifact rail · `⌘\` sidebar · `⌥Space` voice · `⌘,` settings · `⌘1–9` recent threads · `Esc` cancel voice/close rail/dismiss. Windows/Linux: `Ctrl` equivalents.
