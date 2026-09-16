# Component inventory

This file lists every control and surface the desktop shell renders, with the
selector that styles it, its states, and the token each state reads.
`DESIGN.md` holds the laws this inventory serves, and `src/styles/tokens.css`
holds the token values. Every value below comes from a `<style>` rule in
`src/App.svelte`, `src/Launcher.svelte`, `src/lib/*.svelte`, or
`src/styles/*.css`. A value that appears as a literal where a token exists is
marked "raw". A value with no token, such as an opacity or a pixel gap, is
listed as written.

## Shared defaults

These rules apply to every control in a group unless a row below overrides
them.

| Rule | Selector | Value |
| --- | --- | --- |
| Body text | `body` in `base.css` | `--paper` background, `--ink` color, `--font-human`, `--text-15`, `--weight-regular`, `--leading-body`, `--tracking-body` |
| Headings | `h1, h2, h3, h4` in `base.css` | `--weight-semibold`, `--leading-heading`, `--tracking-heading` |
| Link | `a` in `base.css` | `--ink` color, 1px `--border` bottom border, hover `--muted`, transition `--motion-popover --ease-out` |
| Focus ring | `:focus-visible` in `base.css` | `2px solid var(--ink)`, offset 2px. Component rules restate this ring or set `outline-color: var(--ink)`. Two exceptions: `.artifact-divider:focus-visible` and `.sidebar-divider:focus-visible` use offset -2px |
| Selection | `::selection` in `base.css` | `--faint` background, `--ink` color |
| Record fonts | `code, kbd, samp, pre` in `base.css` | `--font-mono`, tabular lining figures |
| Scroll bar | `::-webkit-scrollbar-thumb` in `base.css` | 10px, transparent at rest, `--border` while the region is hovered, `--muted` on thumb hover, `--radius-chip` |
| Reduced motion | `@media (prefers-reduced-motion: reduce)` in `base.css` | Every animation and transition collapses to 0.001ms |
| Shell button | `button` in `App.svelte` | `font: inherit` at `--text-13`, `--ink` on `--surface`, 1px `--border`, `--radius-control`, padding 5px 12px. Hover: border `--muted`. Disabled or `aria-disabled` with `.inactive`: `--muted` color |
| Quiet button | `.quiet` in `App.svelte` and `AccessPanel.svelte` | Transparent background and border over the shell button |
| Primary button | `.primary` in `App.svelte` and `Onboarding.svelte` | `--ink` background and border, `--paper` color. Inactive `.composer-actions .primary[aria-disabled="true"]`: `--faint` background, `--border` border, `--muted` color |
| Access panel button | `button` in `AccessPanel.svelte` | Same as the shell button. Hover: border `--muted` |
| Onboarding button | `button` in `Onboarding.svelte` | Same as the shell button with min-height 28px and padding 5px 10px. Disabled: `--muted` color |

## Frame and title bar

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| Workspace frame | `.workspace` | Grid of title row, sidebar, thread, and rail on the paper frame | `--paper`, 8px frame, `--titlebar-height` title row (28px, 32px on macOS), `--sidebar-column` sidebar column (195px by default, resizable 160px to 420px, kept per device) | Sidebar collapsed: zero column, and the thread panel takes the gap with a `margin-left` slide of `180ms ease`. Rail open: adds `--artifact-rail-width`. Column change transitions `180ms ease` (raw motion, no token at 180ms). `.artifact-resizing` and `.sidebar-resizing` remove the transition |
| Panels | `.sidebar, .thread-panel, .artifact-rail` | The three surfaces inside the frame | `--surface`, 1px `--border`, `--radius-panel` | None |
| Title bar | `.titlebar` | Drag region with the app row | `--paper`, `--text-13`, padding 0 12px | macOS: subgrid with a 78px inset for the traffic lights, and the 32px row centers its 24px controls on the 14pt lights at 16pt |
| Title bar controls | `.titlebar button, .titlebar input` | Size floor for the sidebar toggle and the rename field | 24px min width and height, padding 0 6px | Hover: `--faint` background, transparent border. Focus-visible: global ring |
| Row control | `.row-control` in `RowControl.svelte` | The one quiet row control that New thread, the thread title and Artifacts render through | Transparent, 1px transparent border, `--radius-control`, `--ink`, `font: inherit`, gap 6px, 24px min width and height, padding 0 6px | Hover: `--faint` background. Focus-visible: 2px `--ink` ring at offset 2px. Disabled: `--muted` |
| Sidebar toggle | `.quiet.side-toggle` | Collapses or expands the sidebar | Quiet button with a `LucideIcon` in `--muted` | Hover or focus-visible: `--faint` background, icon `--ink` |
| New thread | `.row-control.new-thread` | Starts a thread | Row control, `plus` icon, label and `kbd` chip, no wrap | Disabled while a run is active |
| Title bar kbd | `.titlebar kbd` | Shortcut chip, `⌘ N` with one space | `--faint` background, 1px `--border`, `--radius-chip`, padding 0 4px, `--text-12 --font-mono`, `--muted` | Stays legible over the row control's `--faint` hover |
| Thread title button | `.row-control.thread-title` | Opens rename | Row control, `font-weight: 600` (raw, `--weight-semibold` exists), ellipsis, no wrap | Disabled with no thread: `--ink`, opacity 1 |
| Thread title input | `input.thread-title` | Renames the thread | Title bar control register, no border, transparent, `--ink`, `font-weight: 600`, ellipsis, flex basis 320px, text selectable | Focus-visible: global ring |
| Update slot | `.update-slot` | Reserved 24px slot for the update control | 24px by 24px, empty | None |
| Artifacts toggle | `.row-control.artifacts-toggle` | Opens the rail | Row control, label and `kbd` chip, no wrap, `order: 2` on macOS | `aria-expanded` follows the rail |
| Record toggle | `.row-control.record-toggle` | Opens the record panel, flush right of Artifacts | Row control, label and `kbd` chip (`⌘ K`), no wrap, `order: 3` on macOS | `aria-expanded` follows the panel. Opening it closes the artifact rail |
| Record panel | `.record-panel` | The rail column's second occupant, 480px minimum | Panel surface, grid of header, kind list and detail, padding 14px 16px 16px | `.workspace.record-maximized` gives it the whole frame and hides the sidebar and thread |
| Record header | `.record-header` | `Record`, the company picker, Maximize | One mono row (`--text-13 --font-mono`), `h2` in body type at `--text-15`, hairline below | Maximize is a quiet control with `aria-pressed`; the picker is a native `select` at 24px |
| Kind list | `.record-kinds .record-kind` | One row per kind, name left and summary right | 28px rows, hairlines between, name in body type, summary in `--text-12 --font-mono --muted`; an own kind reads `(own)` | Hover and `aria-pressed`: `--faint` background |
| Record state line | `.record-state` | One line: empty, reading, or the first sentence of a failure | `--muted --text-13 --font-mono` | A failure is `role="alert"` |
| Record table | `.record-grid` in `RecordTable.svelte` | One kind's rows, columns generated from the kind | 28px rows, hairlines, sticky mono headers in `--muted`; typed cells `--text-12 --font-mono`, title and free text in body type; header click sorts and reads `^` or `v` | Row hover `--faint`. A double-click on a typed cell edits it inline with the composer's muted hairline |
| Proposed change | `.record-diff` | The diff under an edited row, one mono line per change, then Commit and Discard | `--text-12 --font-mono`; a warning in `--ochre`, a failure in `--oxide`; Commit is ink on paper | Enter on Commit applies; Escape discards |
| Record view | `.record-view` in `RecordView.svelte` | One record: prose, fields, identities, relations, history | Title `--text-17` 600, prose `--text-15`, section heads and values in mono, a closed relation in `--muted` | A relation's other end is a quiet link that opens that record |
| Kind toolbar | `.record-toolbar` in `RecordPanel.svelte` | Layout switch, state filter, saved-view picker, Save view, Ask | One mono row, `--text-12 --font-mono`; `.record-layout` pair inside one hairline at `--radius-control`, pressed reads `--faint` and `--ink`; selects 24px | Save view opens one input and Propose, then the diff with Commit and Discard |
| Record board | `.record-board` in `RecordBoard.svelte` | One column per state, cards as titles with one mono line | Columns `paper` with hairlines, cards `surface` at `--radius-control`, a column under a drag lifts its hairline to `--muted`, a card in flight fades to 50% | A drop proposes the state change and the diff shows above the columns with Commit and Discard |
| New record form | `.record-form` in `RecordForm.svelte` | Inputs generated from the kind, required first | 28px inputs with hairlines; enum and typed inputs in mono, free text in body type | Propose is disabled until every required field holds a value |
| Artifact divider | `.artifact-divider` | Drag handle between thread and rail | Transparent, width 8px, `border-radius: 0` (allowed exception), `col-resize` cursor | Draws nothing on hover or drag. Focus-visible: 2px `--ink` ring at offset -2px |
| Sidebar divider | `.sidebar-divider` | Drag handle between sidebar and thread, absent while the sidebar is collapsed | Same as the artifact divider | Same as the artifact divider |
| Artifact rail | `.artifact-rail` | Rail body | Panel surface, padding 22px 24px | Header has 1px `--border` bottom, `h2` at `--text-17` |
| Artifact empty state | `.artifact-empty p` | One line, no illustration | `--muted`, centered | None |
| Drop affordance | `.drop-affordance` | Overlay while files drag over the thread | `color-mix` of `--paper` at 92%, 1px dashed `--muted`, `--radius-panel`, `--ink` text, `--text-12 --font-mono` support line in `--muted` | None |

## Sidebar

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| Sidebar | `.sidebar` | Thread list and settings foot | Panel surface, padding 14px 10px 10px | Collapsed: gone. Zero width, no padding, no hairline, nothing inside |
| Section label | `.side-label` | Heads the thread list | `--muted`, `--text-12 --font-mono`, margin 2px 8px 5px | None |
| Thread row | `.thread-row` | Opens a thread | Transparent, 1px transparent border, `--radius-control`, `--text-13`, `--ink`, padding 7px 8px, gap 9px | Hover: `--faint`. `aria-disabled` while a run is active: opacity .55 |
| Current thread | `.thread-row.active-thread` | Marks the open thread | `--faint` background, 5px round `--ink` dot (allowed 50% radius exception) | None |
| Row time | `.thread-row time` | Relative age, never pushed out of the row | `--muted`, `--text-provenance --font-mono`, `flex: none` | None |
| Row title | `.thread-row-title` | The thread name, faded over its last 28px instead of clipped with an ellipsis | `mask-image` gradient to transparent | None |
| Thread menu | `.thread-menu` | Right-click, Control-click or context menu key on a row, placed at the pointer | `--surface`, 1px `--border`, `--radius-control`, `--shadow-overlay`, padding 4px, min-width 120px | Closes on Escape, Tab, a pointer press outside it or focus leaving it |
| Thread menu item | `.thread-menu button` | Delete, opens the delete confirm | Quiet, 24px floor, `--ink`, `--text-13`, padding 3px 8px | Hover: `--faint`. Disabled while a run is active. Focus-visible: `outline-color: var(--ink)` |
| Delete confirm group | `.thread-delete-confirm` | Inline Delete and Cancel | `--surface`, `--radius-control`, `--ink`, `--text-12 --font-mono`, padding 5px 7px | Buttons: quiet, 24px floor, `font: inherit`. Hover: `--faint`. Disabled while `deletePending` |
| Older threads | `.older-threads` | Loads the next page | Quiet, full width, `--muted`, margin-top 4px | Disabled while loading |
| Settings control | `.side-action` | Foot control that expands the menu | Transparent, gap 9px, padding 7px 8px, `settings` icon at 18px in `--muted`, `--text-13`, `aria-keyshortcuts` and tooltip carry ⌘, or Ctrl+, | Hidden with the collapsed sidebar. The shortcut expands the sidebar with the menu open, and toggles the menu when the sidebar shows |
| Settings block | `.settings-block` | Foot of the sidebar | 1px `--border` top, padding-top 8px, `margin-top: auto` | None |

## Settings menu

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| Menu | `.settings-menu` | Menu the Settings control expands above itself | Grid, gap 12px, padding 6px 8px 12px, no background of its own | Focus-visible: global ring |
| Menu label | `.settings-label` | Heads Home and Account | `--muted`, `--text-12 --font-mono`, uppercase, `letter-spacing: .04em` | None |
| Home path | `.settings-path` | Shows the Home folder | `--muted`, `--text-12 --font-mono`, wraps anywhere | None |
| Change folder | `.settings-home button` | Opens Home settings | Shell button, min-height 28px, padding 4px 10px | Hover: border `--muted` |
| Sign in for cloud features | `.settings-account button` | Local mode account control | Shell button, min-height 28px, padding 4px 10px | Disabled while a run is active or entry is pending |
| Appearance | `Appearance.svelte` | Theme segmented control | See the appearance table | None |

## Thread surface

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| Thread | `.thread` | Scrolling transcript region | 760px column, padding 42px 0 | `.scrolling`: scroll thumb `--border` |
| Empty state | `.empty` | One line before the first message | `--muted`, centered, margin-top 18vh | None |
| Latest | `.latest` | Jumps to the newest message | `--surface`, `--radius-control`, `--muted`, `--text-12 --font-mono`, `--shadow-overlay` | Shown only when content sits below the fold |
| User message | `.user-message` | The user's turn | `--faint`, `--radius-panel`, padding 9px 13px, max-width 78%, wraps anywhere | Missing prompt: `.missing-prompt` in `--muted`, `--text-12 --font-mono` |
| Saved attachments | `.message-attachments li` | Attachment rows under a user turn | 1px `--border`, `--radius-chip`, `--muted`, `--text-12 --font-mono`, padding 5px 8px | Media type `strong` at `--text-12`, `font-weight: 400` (raw, `--weight-regular` exists) |
| Attachment delivery rule | `.attachment-delivery-rule` | One line under the list | `--muted`, `--text-12 --font-mono` | None |
| Prompt storage notice | `.prompt-storage-notice` | Collapsed details above a response | `--muted`, `--text-12 --font-mono`, summary 24px tall | Summary hover: `--ink` |
| Response | `.response` | The model's turn, no bubble | Plain on the panel, margin-bottom 34px | None |
| Response prose | `.response-prose` | Streaming or paused text | max-width 92%, pre-wrap, wraps anywhere | `.streaming` positions the rule and caret |
| Streaming rule | `.streaming-rule` | Underline on the active line | 2px `--signal` | Allowed signal use |
| Caret | `.caret` | Caret on the active line | 2px right border `--signal`, `blink 800ms step-end infinite` | Allowed signal use |
| Thinking | `.thinking` | The mark in its thinking state with the word Routing | `--muted`, `--text-12 --font-mono`, gap 9px, 17px mark | `.thinking path`: `--signal` fill, `breathe 1.8s ease-in-out infinite`. Reduced motion: none |
| Assistant markdown | `AssistantMarkdown.svelte` | Rendered complete reply | See the assistant markdown table | None |
| Provenance | `.provenance` | Receipt line under a reply, toggles the record when it holds a second row | Transparent button, 24px floor, `--muted`, `--text-provenance` at line-height 1.45 (no token) in `--font-mono`, tabular figures, wraps anywhere | Hover: `--ink`. `.route-segment`: `--signal` (allowed). Unavailable: a `p.provenance` with the same register |
| Receipt marker | `.receipt-marker` | Chevron on the provenance line. A one-row receipt shows a 12px `clock` icon in its place on a plain `p.provenance` that never expands | 5px box, 1px `currentColor` edges, rotated -45deg, transition `transform 120ms ease` (raw, `--motion-popover` exists) | `.expanded`: 45deg. Reduced motion: no transition |
| Receipt record | `.receipt-record` | Expanded receipt list, plain under the provenance line | 329px, no border, no padding, `--muted`, `--text-12 --font-mono`, 88px label column | `.route-value`: `--signal` (allowed). `dd` wraps anywhere |
| Message actions | `.message-actions` | Copy row under a complete reply | opacity 0, transition `opacity 120ms ease` (raw, `--motion-popover` exists), gap 2px | Response hover or focus-within: opacity 1 |
| Copy | `.message-actions button` | Copies the reply | Quiet, `--muted`, `--text-12`, gap 5px, 14px `action` icon | Hover: `--faint`, `--ink`. Focus-visible: `outline-color: var(--ink)`. Disabled: opacity .45 |
| Run error | `.run-error` | Failure, stop, or interruption line | `--muted`, `--text-12 --font-mono`, wraps anywhere | `.copy-failure` adds margin-top 4px |
| Run error retry | `.run-error button` | Try again or Resume beside the line | Transparent background, 24px floor, padding 2px 6px, `font: inherit` | Disabled while a run is active, dictation is busy, or an upgrade is pending |
| History error | `.history-error` | Failed history load | `--muted`, `--text-12 --font-mono`, wraps anywhere | Retry uses the shell button |

## Tool card and request cards

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| Tool card | `.tool-card` | Inline mono record of tool activity | 1px `--border`, `--radius-control`, `--surface`, `--muted`, `--text-13 --font-mono`, padding 8px 12px | None |
| Tool row | `.tool-row` | One tool | 20px min height, gap 8px, 7px round dot in `currentColor` (allowed 50% radius exception) | `.tool-running`: `--signal` (allowed), dot `tool-pulse 1.4s ease-in-out infinite`. `.tool-failed .tool-status`: `--oxide` |
| Tool name | `.tool-name` | External tool name | Wraps anywhere | None |
| Parallel group | `.tool-card.tool-group` | Grouped rows under a title | Tool card with `.tool-group-title` in `--muted`, rows 4px apart | None |
| Permission card | `.permission-card.tool-card` | Confirm, select, input, editor, or code-diff request | Tool card in `--ink`, `strong` at `font-weight: 600` (raw, `--weight-semibold` exists), `p` in `--muted` with pre-wrap and wraps anywhere | Error: `.run-error` with `role="alert"` under the actions |
| Permission field | `.permission-field` | Input request | Full width, 1px `--border`, `--radius-control`, `--surface`, `--ink`, `font: inherit`, padding 7px 9px, no outline | Focus: border `--muted`. Placeholder `--muted`. Disabled while the answer is pending |
| Permission editor | `.permission-field.permission-editor` | Editor request | Field plus min-height 84px, vertical resize | Hint `.permission-editor-hint` in `--muted`, `--text-12 --font-mono` |
| Permission actions | `.permission-actions button` | Deny, Allow, option, Submit, or Apply | Shell button, padding 4px 8px, `font: inherit` | Approve group sits flush right. Disabled while pending |
| Applied diff | `.applied-diff.tool-card` | Record of applied file changes | Tool card in `--ink` with the same `strong` and `p` rules as the permission card | Missing record: one `p` sentence |
| Code diff | `CodeDiff.svelte` | Stored diff inside a card | See the code diff table | None |

## Composer

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| Composer | `.composer` | Input surface at the foot of the thread | 760px column, `--surface`, 1px `--border`, `--radius-panel`, padding 12px | Focus-within: border `--muted` |
| Composer textarea | `textarea` in `App.svelte` | Draft text | Transparent, no border or outline, `--ink`, `font: inherit` at body size | Disabled while resuming or switching threads. Placeholder changes to the resuming line |
| Selected files | `.attachments li` | Files chosen before Send | 1px `--border`, `--radius-chip`, `--muted`, `--text-12 --font-mono`, padding 4px 6px 4px 9px | Remove button: no border, transparent, inherits color, `--text-12` |
| Composer row | `.composer-row` | Band under the input | `--muted`, `--text-12`, gap 8px, margin-top 8px | None |
| Composer hint | `#composer-hint` | One status line in the band, absent in flight | Inherits the row register | Thread switching and dictation replace the hint |
| Model chip | `.quiet.model-chip` | Model source chip, opens the model panel | 1px `--border`, `--radius-chip`, `--ink`, `--text-12 --font-mono`, 24px floor, padding 2px 8px | Hover: `--faint`. `aria-expanded` follows the panel |
| Voice | `.composer-actions .quiet` | Hold or toggle dictation | Quiet button | `aria-pressed` while dictating. Disabled while a run is active or dictation finishes |
| Add files | `.composer-actions .quiet` | Opens the file picker | Quiet button | Hidden while a run is active |
| Composer action | `.composer-action` | The band's one action control at its right end, absent while the draft is empty | 24px floor, padding 4px, one `action` icon | `.primary` with `arrow-up` once the draft has text: sends, tooltip `Send (⏎)`. Inactive: `--faint`, `--border`, `--muted`. `.stop` with `square` while a reply is in flight: transparent, 1px `--border`, `--muted`, cancels the run, tooltip `Stop the reply`. `aria-disabled` with a transparent border while the run is pending or resuming. Disabled while switching threads |
| Cancel error | `.cancel-error` | Submit, cancel, or queue failure | `--muted`, `--text-12 --font-mono`, margin-bottom 8px | None |
| Update notice | `.update-notice` | Runtime upgrade in progress | Grid, gap 2px, `.record` line and `.support` line at `--text-12` | None |

## Model panel

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| Model panel | `.model-panel` | Popup over the composer for provider settings | 380px, `--surface`, 1px `--border`, `--radius-panel`, `--shadow-overlay`, padding 14px 16px 16px, max-height 60vh | Focus-visible: global ring |
| Panel head | `.model-panel-head` | Title and Close | Close is a quiet shell button with a 24px floor, padding 2px 8px | None |
| Provider choice | `.provider-choice label` | Radio row per provider | 32px min height, `--radius-control`, `--ink`, `font: inherit`, gap 8px, legend in `--muted` `--text-12 --font-mono` | Hover: `--faint`. Fieldset disabled: opacity .55. Radio: 24px, `accent-color: var(--ink)`, global focus ring |
| Provider tag | `.provider-tag` | Saved or Not set | `--muted`, `--text-12 --font-mono` | None |
| Panel input | `.model-panel > input` | Key or server URL | `--paper`, 1px `--border`, `--radius-control`, `--ink`, `font: inherit`, padding 6px 8px | Disabled while a run is active or a save is pending. Label in `--muted` `--text-12 --font-mono` |
| Panel status | `.model-panel .support` | Save result | `--text-12 --font-mono`, `--muted` | None |

## Dictation and speech install

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| Capture status | `.capture-status` | Listening line in the band | `--font-mono`, gap 8px | None |
| Capture meter | `.capture-meter i` | Five 2px bars | `--muted`, heights 6, 10, 14, 10, 6px, `capture 900ms ease-in-out infinite alternate` | Reduced motion: no animation, bars rest at full height |
| Dictation error | `.dictation-error` | Dictation failure line | `--muted`, `--text-12 --font-mono`, margin-top 7px | None |
| Speech install card | `.speech-install-card` | Install facts and progress | 1px `--border`, `--radius-control`, `--surface`, `--ink`, `--text-12 --font-mono`, padding 10px 12px, `strong` at `font-weight: 600` (raw, `--weight-semibold` exists) | `dt` in `--muted`, `dd` wraps anywhere. Error `.speech-install-error`: `--oxide` |
| Install progress | `.speech-install-progress progress` | Download bar | 6px, `accent-color: var(--muted)` | None |
| Install buttons | `.speech-install-card button` | Install, Cancel install, Try again | Shell button, padding 4px 8px, `font: inherit`, margin-top 7px | Disabled while pending |
| Close card | `.speech-install-card .close-card` | Dismisses the card | Quiet, 24px floor, `--muted`, 14px `x` icon | Hover: `--ink` |
| Speech install notice | `.speech-install-notice` | One line after dismiss | `--muted`, `--text-12 --font-mono`, padding 0 12px | None |

## Toasts and notices

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| Entitlement toast | `.entitlement-toast` | Access change notice | Fixed bottom 24px, `--surface`, 1px `--border`, `--radius-control`, `--ink`, `--shadow-overlay`, `toast-enter` at `--motion-popover --ease-out` | None |
| Auth state | `.auth-state` | Sign-in, error, and background service notices | Centered column, gap 12px, max-width 420px, margin-top 34px | None |
| Support line | `.support` | Secondary sentence | `--muted` | None |
| Record line | `.record` | Mono notice or error | `--font-mono`, `--text-12`, `--muted`. `.error-record` adds `--leading-body` | None |
| Sign-in link | `.sign-in-link` | Opens the sign-in page | `--ink`, `--text-12`, underline | Hover from the base link rule |
| Sign in and Use local mode | `.auth-actions button` | Entry controls | Primary and shell buttons, gap 8px | `.inactive` while signing in: `--muted` |
| Lockup | `.lockup` | Seal and wordmark on the launch screen | 34px seal in `--ink`, `.name` at `--text-28`, `font-weight: 600` (raw, `--weight-semibold` exists), `letter-spacing: -0.01em` (raw, `--tracking-heading` exists), `--leading-body` | None |
| Version | `.meta` | Shell version under the lockup | `--font-mono`, `--text-12`, `--muted`, margin-top 18px | None |

## Access panel

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| Profile block | `.profile-block` | Foot of the sidebar for a signed-in user | 1px `--border` top, padding-top 10px | None |
| Profile button | `.profile-button` | Opens the popover | Transparent, full width, gap 9px, padding 9px 8px, name at `--text-13`, details in `--muted` `--text-12 --font-mono` | Hover: border `--muted`. `aria-expanded` follows the popover |
| Access popover | `.access-popover` | Profile dialog above the button | 330px, `--paper`, 1px `--border`, `--radius-control`, `--shadow-overlay`, no outline | Focus-visible: border `--muted` |
| Popover header | `.access-popover header` | Name, details, Close | 1px `--border` bottom, padding 14px, `h2` at `--text-13`, `p` in `--muted` `--text-12 --font-mono` | Close `.close-access`: quiet, 24px floor, `--text-17` |
| Section label | `.access-label` | Heads each section | `--muted`, `--text-12 --font-mono`, uppercase, `letter-spacing: .04em` | None |
| Section | `.retention-section, .entitlements-section, .devices-section, .companions-section, .voice-section` | Dividers between sections | margin-top 14px, padding-top 12px, 1px `--border` top | None |
| Retention help | `.retention-help` | One sentence | `--muted`, `--text-12` | None |
| Retention option | `.retention-options label` | Radio row | 24px min height, gap 7px, `--text-12` | Radio: 24px floor, `accent-color: var(--ink)`. Disabled while saving |
| Access status | `.access-status` | Loading, error, or notice line with a control | `--muted`, `--text-12 --font-mono`, margin 8px 0 | Try again, Restart Muniment, and Close this window use the panel button |
| Empty grant | `.empty-grant` | One-line empty state | `--muted`, `--text-12 --font-mono` | None |
| Grant toggle | `.group-toggle` | Expands a grant | Transparent, no border, `--ink`, `--text-12 --font-mono`, padding 9px 2px, 1px `--border` top on `.access-group` | `aria-expanded` swaps the + and - glyph |
| Grant grid | `.grant-grid` | Grant details | Grid, gap 10px, `h3` in `--muted`, values at `--text-12 --font-mono`, wraps anywhere | None |
| Access note | `.access-note` | Closing sentence | `--muted`, `--text-12` | None |
| Device row | `.device-list li` | One device | padding 9px 2px, 1px `--border` top after the first, `strong` at `font-weight: 600` (raw, `--weight-semibold` exists) | `.revoked strong`: `--oxide`, `font-weight: 400` (raw, `--weight-regular` exists), line-through |
| Current device chip | `.current-device` | This device | 1px `--border`, `--radius-chip`, `--faint`, `--muted`, `--text-12 --font-mono`, padding 1px 5px | None |
| Device state | `.device-state` | Active or Revoked | `--muted`, `--text-12 --font-mono`, flush right | The word carries the state |
| Device time | `.device-list time` | Last active | `--muted`, `--text-12 --font-mono` | None |
| Companion row | `.companion-list li` | One connected program | padding 9px 2px, 1px `--border` top after the first, heading at `--text-12` with `strong` at `font-weight: 600` (raw) | Hover or focus-within hides the kind label and reveals Revoke |
| Companion revoke | `.companion-revoke` | Opens the revoke confirm | Absolute, 24px floor, transparent border, `--paper`, `--muted`, `--text-12 --font-mono`, opacity 0, transition `opacity 120ms ease` (raw, `--motion-popover` exists) | Row hover or focus-within: opacity 1. Hover: `--faint`, `--ink`. Focus-visible: `outline-color: var(--ink)` |
| Companion confirm | `.companion-confirm` | Inline Revoke and Cancel | `--ink`, `--text-12`, `strong` at `font-weight: 600` (raw) | Buttons: quiet, `--text-12 --font-mono`, padding 3px 6px, hover `--faint`. Disabled while pending. Error `.revoke-error` in `--muted` |
| Shortcut help | `.shortcut-help` | One sentence | `--muted`, `--text-12` | None |
| Shortcut capture | `.shortcut-capture` | Shows and records the voice shortcut | Panel button, full width, `--font-mono`, `small` in `--muted` `--text-12 --font-human`, padding 8px 9px | Capturing: label reads the pending shortcut. Disabled while changing |
| Shortcut actions | `.shortcut-actions button` | Cancel and Apply | Panel buttons, gap 6px | Apply disabled without a pending shortcut |
| Restore default | `.quiet.restore-shortcut` | Resets the shortcut | Quiet, `--muted`, padding 3px 0 | Disabled at the default |
| Shortcut error | `.shortcut-error` | Capture failure | `--oxide`, `--text-12` | None |
| Footer and sign out | `.access-footer .quiet.sign-out` | Sign out | 1px `--border` top, padding 9px 14px, quiet `--muted` button | Hover: border `--muted` |

## Appearance

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| Segmented control | `.theme-options` | System, Light, Dark | Inline flex, 1px `--border`, `--radius-control` | None |
| Segment | `.theme-options button` | One theme | Transparent, `border-radius: 0` (allowed exception), `--muted`, `--text-13`, padding 5px 12px, 1px `--border` between segments. First and last segments compose `--radius-control` on their outer corners | Hover or `aria-pressed="true"`: `--faint`, `--ink`. Focus-visible: global ring, z-index 1 |

## Confirm dialog

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| Backdrop | `.dialog-backdrop` | Modal scrim for a pairing request | `color-mix` of `--ink` at 28%, padding 24px | None |
| Panel | `.dialog-panel` | Dialog body | 440px, `--surface`, 1px `--border`, `--radius-panel`, `--shadow-overlay`, `--font-human`, padding 24px | `h2` at `--text-22` with `--leading-heading` and `--tracking-heading`. Copy in `--muted` at `--text-15`, `--leading-body`, wraps anywhere |
| Deny | `.dialog-actions button` | Refuses the request, takes focus first | Transparent, 1px `--border`, `--radius-control`, `--ink`, `--weight-semibold --text-13 --font-human`, min-height 36px, padding 7px 18px | Hover: `--faint`. Disabled: opacity .55 |
| Allow | `.dialog-actions .allow` | Approves the request | `--ink` background and border, `--paper` color | Hover: opacity .9. Disabled: opacity .55 |

## Launcher

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| Launcher form | `.launcher` | The 600 by 80 window, one composer line | `--surface`, 1px `--border`, padding 0 24px | Focus-within: border `--muted` |
| Launcher input | `input` in `Launcher.svelte` | First message of a new thread | 48px tall, transparent, no border or outline, `--ink`, `font: inherit` at `--text-17`, placeholder `--muted` | Read-only and `aria-busy` while pending. Error: `aria-invalid` with a hidden alert |

## Onboarding

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| First run section | `.onboarding` | Column under the lockup | 760px, margin-top 48px, padding 2px | Under 600px tall: margin-top 12px |
| First run composer | `.composer` in `Onboarding.svelte` | Composer before Home exists | `--surface`, 1px `--border`, `--radius-panel`, padding 16px | Focus-within: border `--muted` |
| First run textarea | `textarea` in `Onboarding.svelte` | First message | Transparent, no border, `--ink`, `--text-15 --font-human`, `line-height: 1.55` (raw, `--leading-body` exists), min-height 90px, vertical resize | Placeholder `--muted` |
| First run row | `.composer-row` | Hint and Send | `--muted`, `--text-12`, gap 12px, margin-top 12px | Send is the primary button, `aria-disabled` while busy |
| Chips | `.chips button` | Model source, Home path, scan result | Onboarding button, `--radius-chip`, `--text-12 --font-mono`, wraps anywhere | `aria-expanded="true"`: border `--muted`, `--faint`. `.home-chip` clips with an ellipsis |
| Panel | `.panel` | Panel a chip opens | `--surface`, 1px `--border`, `--radius-panel`, padding 16px, margin-top 16px, scrolls | `h2` at `--text-15`, `font-weight: 600` (raw, `--weight-semibold` exists) |
| Settings heading | `h1` in `Onboarding.svelte` | Home settings title | `--text-22` | None |
| Panel copy | `p` in `Onboarding.svelte` | Panel sentences | `--text-13`, wraps anywhere | None |
| Path and scan rows | `.path, li, .error` | Mono records | `--text-12 --font-mono`. `li`: column, gap 4px, padding 10px 0, 1px `--border` top | `.note, .error`: `--muted` |
| Settings actions | `.actions button` | Change folder, Cancel, Save Home | Onboarding buttons, gap 12px, primary flush right | Disabled while busy or picking |

## Icons

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| Lucide icon | `.lucide` in `LucideIcon.svelte` | Vendored Lucide path data on a 24 grid | 16px default, `stroke: currentColor`, `stroke-width: 1.6`, round caps and joins | None |
| Side icon | `.side-icon` | Icon on the rail and rows | `--muted` | Parent hover or focus-visible sets `--ink` |
| Action icon | `.action-icon` | Icon inside a control | `color: inherit` | Follows the control |

## Code diff

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| Code diff section | `.code-diff` | Stored diff, sized by its own width | `--ink`, `--text-13 --font-mono`, container query root | Under 480px wide: sides stack |
| Warning | `.code-diff-warning` | Truncation notice | 1px `--border`, 3px `--ochre` left border, `--radius-control`, `--surface`, `--ink`, padding 10px 12px | None |
| Empty and binary | `.code-diff-empty, .code-diff-binary` | Empty message or binary file record | 1px `--border`, `--radius-control`, `--surface`, padding 10px 12px | Binary path in `--ink`, message in `--muted` |
| File wrapper | `.code-diff .d2h-file-wrapper` in `code-diff.css` | diff2html file frame | `--border`, `--radius-control`, `--surface` | None |
| Header and gutters | `.d2h-file-header, .d2h-code-linenumber, .d2h-info` | File name row and line numbers | `--faint`, `--border`, `--muted` | None |
| Insert | `.d2h-ins` | Added line | `--signal-soft`, border `color-mix` of `--signal` at 30% | Changed span: `--signal` at 24% |
| Delete | `.d2h-del` | Removed line | `color-mix` of `--oxide` at 12% over `--surface`, border `--oxide` at 30% | Changed span: `--oxide` at 24% |
| Labels | `--d2h-*-label-color` | Insert, delete, change, moved | `--signal`, `--oxide`, `--ochre`, `--muted` | None |
| Side panel | `.d2h-file-side-diff` | Scroll region | Tab stop and group role when it overflows | Focus-visible: global ring |

## Assistant markdown

| Component | Selector | Purpose | Rest | States |
| --- | --- | --- | --- | --- |
| Container | `.assistant-markdown` | Rendered complete reply | max-width 92%, `--ink`, pre-wrap, wraps anywhere | None |
| Headings | `h2, h3, h4, h5` | Reply headings | `--text-22`, `--text-17`, `--text-15`, `--text-15` | None |
| Blockquote | `blockquote` | Quoted text | 1px `--border` left, `--muted`, padding-left 12px | None |
| Rule | `hr` | Divider | 1px `--border` top | None |
| Link | `a` | External link | `--ink`, underline in `--border`, offset 2px | Hover: `--muted` text and underline. Focus-visible: global ring |
| Inline code | `code` | Code span | `--faint`, `--radius-chip`, `--text-13 --font-mono`, padding 1px 3px | None |
| Code block | `pre` | Block, scrolls sideways | 1px `--border`, `--radius-control`, `--faint`, inner padding 10px 12px | Tab stop and group role when it overflows. Focus-visible: global ring |
| Table | `table, th, td` | Block table, scrolls sideways | 1px `--border` cells, padding 6px 10px, `th` on `--faint` at `--weight-semibold` | Focus-visible: global ring |

## Raw values where a token exists

| Selector | Raw value | Token |
| --- | --- | --- |
| `.name`, `.thread-title`, `.permission-card strong`, `.applied-diff strong`, `.speech-install-card strong`, `.device-heading strong`, `.companion-heading strong`, `.companion-confirm strong`, Onboarding `h2` | `font-weight: 600` | `--weight-semibold` |
| `.message-attachments strong`, `.revoked .device-heading strong` | `font-weight: 400` | `--weight-regular` |
| `.name` | `letter-spacing: -0.01em` | `--tracking-heading` |
| Onboarding `textarea` | `line-height: 1.55` | `--leading-body` |
| `.companion-revoke`, `.message-actions`, `.receipt-marker` | `120ms ease` | `--motion-popover` and `--ease-out` |

Values with no token stay as written: the 180ms grid slide, the 800ms blink,
the 1.8s breathe, the 1.4s tool pulse, the 900ms capture meter, the .04em
label tracking, the 1.45 provenance line height, and every opacity.

## Rules the tests hold

| Test | Rule |
| --- | --- |
| `signal-allowlist.test.js` | `--signal` appears only on the listed App selectors, never in markup, every component focus ring is 2px `--ink` at offset 2, and base.css collapses all motion under reduced motion |
| `target-size.test.js` | `.thread-menu button`, the delete confirm buttons, `.run-error button`, `.provenance`, `.companion-revoke`, `.close-access`, and the retention radios hold a 24 by 24 minimum |
| `control-name.test.js` | Every input and textarea in App and Onboarding carries `aria-label`, `aria-labelledby`, or a `label for` |
| `shape-scale.test.js` | Every radius is one of the three radius tokens and every shadow one of the two depth tokens, except the listed dots, the divider reset, and the segmented control corners |
| `type-scale.test.js` | Every `font-size` and `font` shorthand resolves through a `--text-*` token, with `font: inherit` the one size-free form |
| `record-font.test.js` | `.record`, `.tool-card`, `.provenance`, `.receipt-record`, the onboarding chips, and the onboarding path rows read `--font-mono` |
| `contrast.test.js` | `--ink`, `--muted`, and `--signal` clear 4.5:1 on `--paper`, `--surface`, and `--faint` in all four palettes |
| `text-wrap.test.js` | `.provenance`, `.user-message`, `.response-prose`, `.tool-name`, `.permission-card p`, `.receipt-record dd`, and `.assistant-markdown` set `overflow-wrap: anywhere` |
| `class-usage.test.js` | Every class in markup has a rule in its component or in base.css, and class expressions stay literal |
