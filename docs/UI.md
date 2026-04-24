# Forgehold — UI / UX Specification

**Version:** 0.1 (draft)
**Last updated:** 2026-04-22

> The purpose of this document: pin down the visual language and key
> screens before we write a line of Svelte. Everything below is a
> specification, not an implementation.

---

## 1. Design Philosophy

Five principles Forgehold stands on:

1. **Object-first.** Every card is an object you can pick up and move.
   That's the core mechanic; everything else bends around it.
2. **Direct manipulation.** No "configure step" modals: you drag,
   connect, see the result. Config is inline and minimal.
3. **Calm by default.** Dark background, muted colors, unhurried
   animations. Bright accents only where they matter: running state,
   unread, errors.
4. **Keyboard-first for power users, mouse-first for learning.**
   Anything you can do with the mouse has a shortcut. Anything you can
   do with the keyboard is visible in the UI.
5. **Cinematic micro-interactions.** When you drag an object it's alive
   (slight tilt, drop shadow). When it lands on a drop zone, the zone
   "breathes". When a run starts, glow emanates from the card. This is
   what separates the product from "yet another Electron thing".

### References (for mood, not for copying)

- Arc browser — Little Arc, Boosts, transitions between spaces
- Linear — command palette, information density, typography
- Raycast — command palette structure, fuzzy search
- Granola / Arc Max — dark mode with warm accent
- Things 3 — animations and micro-interactions

---

## 2. Visual Language

### 2.1 Color palette

**Base (dark mode, default):**

```
--bg-0:        #08090B    /* window, deep near-black with cool cast  */
--bg-1:        #0F1114    /* card surface                            */
--bg-2:        #16181D    /* card hover, slightly lifted             */
--bg-glass:    rgba(255, 255, 255, 0.04)  /* translucent overlay    */
--border:      rgba(255, 255, 255, 0.08)
--border-hi:   rgba(255, 255, 255, 0.14)

--text-0:      #F4F5F7   /* primary                                  */
--text-1:      #A8ACB4   /* secondary                                */
--text-2:      #6A6F78   /* tertiary, timestamps, metadata           */
--text-mute:   #474B54
```

**Accent per Source (an important signature):**

```
--accent-jira:   #2684FF   /* Atlassian blue                            */
--accent-github: #8B5CF6   /* violet (not the literal GitHub purple)    */
--accent-slack:  #7C3AED → #EC4899 gradient (branded but unified)       */
--accent-claude: #D97757   /* Anthropic warm orange                      */
--accent-forge:  #EA580C → #F59E0B gradient (brand)
```

The accent shows up as: thin 2px stripe on the left of a card, source
icon, glow around a running card, background of an empty drop zone when
a compatible object is over it.

**Semantic:**

```
--success:  #10B981
--running:  #F59E0B    /* same as forgehold brand — makes running "ours" */
--error:    #EF4444
--info:     #3B82F6
```

### 2.2 Typography

- **Primary:** Inter (v4 variable weight), fallback SF Pro.
- **Mono:** JetBrains Mono or Geist Mono (code, IDs, time).
- **Tracking:** -0.02em on h1/h2, 0 on body.
- **Sizes:**

```
--t-display:  28px / 32px (dashboard header)
--t-h1:       20px / 28px
--t-h2:       16px / 22px
--t-body:     14px / 20px
--t-small:    12px / 16px  (metadata)
--t-mono:     12px / 18px
```

### 2.3 Spacing / radii

4px step. Radii: 6 (chip), 10 (card), 14 (panel), 20 (modal).

### 2.4 Elevation

No explicit shadows on a dark background. Elevation comes from:
- a lighter background (bg-0 → bg-1 → bg-2)
- a subtle top highlight: `box-shadow: inset 0 1px 0 rgba(255,255,255,0.04)`
- blur + tint for "floating" surfaces

### 2.5 Motion

- **Spring presets:**
  - `snappy`: stiffness 400, damping 30 (drag, hover)
  - `smooth`: stiffness 180, damping 22 (panel transitions)
  - `gentle`: stiffness 90, damping 20 (enter/exit)
- **Durations:** 120ms (micro), 220ms (panels), 350ms (reveals).
- **Never linear.** Everything is spring or cubic-bezier with character.

---

## 3. Layout & Surfaces

### 3.1 Main window

Default 1440×900, minimum 960×640. Traditional desktop chrome, no custom
titlebar (overkill for MVP).

### 3.2 Three surfaces

**Dashboard** — the landing screen. Inbox on the left, quick actions in
the middle, active runs top-right.

**Workbench** — the "operating table". Objects, drop zones, workflow.
The primary screen.

**Command Palette (⌘K)** — floats on top. Fuzzy search, quick actions,
recent objects.

Switching Dashboard ↔ Workbench is a sidebar tab or ⌘1 / ⌘2.

---

## 4. Wireframes

### 4.1 Main workbench (primary screen)

```
╔══════════════════════════════════════════════════════════════════════════════════╗
║                                                                                  ║
║  ⌘ forgehold ⌂ acme-team ▾        ⌕  search or press ⌘K            ⚡ 2   👤    ║
║                                                                                  ║
╠══════════╤══════════════════════════════════════════════╤════════════════════════╣
║          │                                              │                        ║
║  INBOX   │          ━━━━━━  WORKBENCH  ━━━━━━           │        INSPECTOR       ║
║   18  ●  │                                              │                        ║
║          │  ┌────────────────────────────┐              │  ╭──────────────────╮  ║
║  ▸ ⚡Live │  ┃█ PROJ-1234                 │              │  │ PROJ-1234        │  ║
║      2   │  ┃  Fix auth retry loop       │              │  │                  │  ║
║          │  ┃  ● high · bug · in review  │              │  │ ● high · bug     │  ║
║  ▸ ⭐Pin  │  ┃  👤 nik  💬 3  📎 2         │              │  │ Assignee:  @me   │  ║
║      4   │  └──────────┬─────────────────┘              │  │ Reporter: @qa    │  ║
║          │             │ dragging…                      │  │ Updated: 2h ago  │  ║
║  ┌─────┐ │             ▼                                │  │                  │  ║
║  │Jira │ │       ╔═══════════════════╗                  │  │ Linked:          │  ║
║  │ 7   │ │       ║                   ║ ← drop active    │  │  ↳ PR #482 draft │  ║
║  └─────┘ │       ║   ⚙  Claude Code  ║                  │  │  ↳ Incident #17  │  ║
║  ┌─────┐ │       ║   give me a diff  ║                  │  │                  │  ║
║  │ GH  │ │       ║                   ║                  │  │ Last comments:   │  ║
║  │ 4   │ │       ╚═════════╤═════════╝                  │  │ @qa  "can repro  │  ║
║  └─────┘ │                 │                            │  │  on staging"     │  ║
║  ┌─────┐ │                 ▼                            │  │                  │  ║
║  │Slack│ │     ┌──────────────┐   ┌──────────────┐      │  │ ─── Artifacts ── │  ║
║  │ 7   │ │     │ 📤 #eng-chat │   │ 🔔 Remind me │      │  │                  │  ║
║  └─────┘ │     │   send draft │   │   in 2h      │      │  │ No artifacts yet │  ║
║          │     └──────────────┘   └──────────────┘      │  │                  │  ║
║ ─────    │                                              │  ╰──────────────────╯  ║
║          │     ┌──────────────┐   ┌──────────────┐      │                        ║
║  FLOWS   │     │ 💬 Jira note │   │ + new action │      │                        ║
║          │     │   add comment│   │              │      │                        ║
║  ★ ticket│     └──────────────┘   └──────────────┘      │                        ║
║    → PR  │                                              │                        ║
║    → slk │                                              │                        ║
║  ★ daily │                                              │                        ║
║    digest│                                              │                        ║
║  + new   │                                              │                        ║
║          │                                              │                        ║
╠══════════╧══════════════════════════════════════════════╧════════════════════════╣
║                                                                                  ║
║  ●  PROJ-1234 → Claude Code         00:42   streaming… "reading auth.ts"    ✕    ║
║                                                                                  ║
╚══════════════════════════════════════════════════════════════════════════════════╝
```

**Left sidebar (Inbox):**
- Unread counter at the top.
- Live / Pin — quick filters.
- Sources with a count indicator per source.
- Saved Flows at the bottom.

**Center (Workbench):**
- Top: objects the user pulled in from the Inbox (or clicked).
- Middle: active drop zones (Claude Code, Slack, Jira comment, Reminder).
- Bottom: slot for new action chips ("+ new action").

**Right panel (Inspector):**
- Context for the selected object.
- Metadata, relations, recent comments.
- Artifacts (results of runs).

**Bottom bar (Timeline / Active runs):**
- One line per active run.
- Progress, streaming text, cancel button.
- Click expands the full log.

### 4.2 Object card (anatomy)

```
┌─ ← 2px accent stripe (color = source)
│ ┌──────────────────────────────────┐
│ │  PROJ-1234               ·  Jira │  ← external_id + source icon
│ │                                  │
│ │  Fix auth retry loop             │  ← title (t-h2)
│ │                                  │
│ │  ● high · bug · in review        │  ← status chips (t-small)
│ │                                  │
│ │  👤 nik   💬 3   📎 2  · 2h     │  ← meta row
│ └──────────────────────────────────┘
```

**States:**
- Default: bg-1, subtle border
- Hover: bg-2, border-hi, slight scale (1.01)
- Dragging: rotate 2deg, elevated shadow, opacity 0.92, cursor becomes "carry"
- Pinned: gold accent in corner
- Unread: accent dot before the external_id

### 4.3 Drop zone (anatomy)

```
Idle:                  Active (object over):        Invalid object:
┌───────────────┐      ╔═══════════════╗             ┌ ─ ─ ─ ─ ─ ─ ─ ┐
│ ⚙  Claude     │      ║ ⚙  Claude     ║                drop me...
│    Code       │      ║    Code       ║             │  (not supported) │
│               │      ║   release ⬇   ║             └ ─ ─ ─ ─ ─ ─ ─ ┘
│   drop here   │      ╚═══════════════╝               (red-ish, dashed)
└───────────────┘      (solid glowing)
```

- Idle: outlined, text-1 color
- Active (compatible object hovering): pulsating glow in the source's
  accent color; the action icon bounces on a spring animation
- Active incompatible: dimmed, dashed border, short red hint
- Running (after drop): animated gradient border loop, mini progress inside

### 4.4 Running state visualization

When an object is dropped on an action, the card and drop zone "connect"
with an animated line (SVG path with a flowing gradient). Inside the
drop zone: live text (what Claude is writing) and a progress ring.

```
┌──────────────┐       ╔═══════════════╗
│ PROJ-1234    │═══════║ ⚙  Claude     ║
│ Fix auth ... │       ║               ║
└──────────────┘       ║ ▸ reading     ║
                       ║   auth.ts     ║
                       ║ ▸ running     ║
                       ║   tests       ║
                       ║ ◉◉◉◉◉◌◌◌ 62% ║
                       ╚═══════════════╝
```

### 4.5 Command palette (⌘K)

```
     ╭──────────────────────────────────────────────────────╮
     │ ⌕  fix auth|                                          │
     ├──────────────────────────────────────────────────────┤
     │                                                       │
     │  OBJECTS                                              │
     │  ┃  PROJ-1234 · Fix auth retry loop         Jira  ↵  │
     │  ┃  #482      · Refactor auth middleware    GH    ↵  │
     │  ┃  nik: "auth flow broken on stage"        Slack ↵  │
     │                                                       │
     │  ACTIONS                                              │
     │  ⚙  Send selected to Claude Code             ⌘⇧C    │
     │  📤  Post to #eng-chat                         ⌘⇧S    │
     │  🔔  Remind me in 2h                           ⌘⇧R    │
     │                                                       │
     │  WORKFLOWS                                            │
     │  ★  ticket → claude → PR → slack              1     │
     │  ★  daily digest                              2     │
     │                                                       │
     ╰──────────────────────────────────────────────────────╯
```

- Centered modal, 640 × max(60vh).
- Backdrop: bg-0 at 0.72 opacity + 18px blur.
- Groups: Objects, Actions, Workflows, Settings.
- Esc / click-outside closes. ↑↓ navigate. ↵ executes.

### 4.6 Dashboard (landing)

```
╔══════════════════════════════════════════════════════════════════════════════════╗
║  ⌘ forgehold ⌂ acme-team ▾        ⌕  search or press ⌘K            ⚡ 2   👤    ║
╠══════════════════════════════════════════════════════════════════════════════════╣
║                                                                                  ║
║   Good morning, Nik.                          ACTIVE                             ║
║                                               ● PROJ-1234 → Claude      01:24    ║
║   You have 18 unread, 3 flagged,              ● #482 review → digest    00:12    ║
║   2 runs in progress.                                                            ║
║                                                                                  ║
║   ─────── INBOX ────────                      ─────── WORKFLOWS ──────           ║
║                                                                                  ║
║   ┃ PROJ-1234   Fix auth ...     Jira         ★  ticket → claude → slack         ║
║   ┃ #482        Refactor auth    GH              runs 14 times this week         ║
║   ┃ @qa in eng  "stage broken"   Slack                                           ║
║   ┃ INC-17      DB timeout       Jira         ★  daily digest                    ║
║   ┃ #488        Feature: sso     GH               every morning 09:00            ║
║   ...                                                                            ║
║                                                + new workflow                    ║
║                                                                                  ║
║   ─────── QUICK ACTIONS ────────                                                 ║
║                                                                                  ║
║   [ Send top ticket to Claude ]   [ Post standup digest ]   [ Open workbench ]   ║
║                                                                                  ║
╚══════════════════════════════════════════════════════════════════════════════════╝
```

### 4.7 Empty states

| Screen      | Empty state                                          |
|-------------|------------------------------------------------------|
| Inbox       | "Connect Jira / GH / Slack to see objects here"      |
| Workbench   | "Drag an object from the Inbox to get started"       |
| Inspector   | "Nothing selected"                                   |
| Timeline    | hidden (renders only if there are runs)              |

---

## 5. Key Interactions

### 5.1 Drag object → drop zone

1. User mousedown on a card → the card lifts (z + shadow), slight tilt.
2. All compatible drop zones "wake up": glow + pulse.
3. Incompatible ones dim (opacity 0.4).
4. Drop: a quick snap animation sends the object into the zone, and the
   zone transitions to running state.

### 5.2 Compose a workflow visually

1. Drop an object on Claude Code → a run starts.
2. When the artifact is ready, an artifact card appears "under" the
   Claude zone.
3. The user can grab it and drop it on Slack / Jira comment / Reminder.
4. Click "save as workflow" → name modal → saved.

### 5.3 Shortcuts

```
⌘K        Command palette
⌘1        Dashboard
⌘2        Workbench
⌘↩        Send selected object to Claude Code (default action)
⌘⇧S       Send to Slack (choose channel)
⌘⇧R       Set reminder
⌘⇧N       New workflow
⌘F        Search within current view
⌘,        Settings
⌘.        Cancel active run
Space     Preview selected object (inline peek)
j / k     Next / previous in inbox
⌘P        Pin object
```

---

## 6. Accessibility

- All colors checked for WCAG AA (not AAA — a trade-off with aesthetics).
- Focus ring: 2px accent-forge, 2px offset, always visible.
- Every drag operation has a keyboard equivalent: select → ⌘E → menu of
  actions.
- Reduced motion: springs → ease-out, 100ms duration.
- Screen reader: every card has an aria-label from title + source + kind.

---

## 7. Light mode

Deferred to post-MVP. Designed so switching is cosmetic: all colors live
behind CSS vars, no component hardcodes a hex.

---

## 8. Component inventory (for implementation)

Minimum set of reusable components to build first:

- `<ObjectCard>` — draggable, with source accent
- `<RepoCard>` — row in the Repositories view (branch + status + quick actions)
- `<DropZone>` — accepts `ObjectKind[]`, shows states
- `<SidebarList>` — Inbox, Flows, Repositories
- `<CommandPalette>` — ⌘K modal
- `<RunStrip>` — bottom bar one-line run indicator
- `<InspectorPanel>` — right panel
- `<Chip>` — status / kind / label
- `<StatusBadge>` — sync state (clean/dirty/ahead/behind)
- `<Button>` — primary / secondary / ghost variants
- `<GlowBorder>` — wrapper for running/active-state elements
- `<MarkdownEditor>` — for rules editing (with scope selector)
- `<RulesPreview>` — shows the resolved prompt prefix

---

## 9. Resolved UI questions

- **Multi-select in Inbox:** ✅ Yes, but only for bulk-safe actions (pin,
  reminder, archive). Multi-drop onto Claude Code is forbidden.
- **Workflow composing:** ✅ Inline in the center of the Workbench. For
  complex branching in post-MVP we plan a separate canvas view.
- **Peek preview:** ✅ Space only (not hover). Hover does a light
  highlight of relations (linked PR, linked ticket).
- **Team vs personal indicator:** ✅ A small avatar overlay in the
  corner for shared; a separate "Personal" sidebar section once team
  mode is on.

---

## 10. Repositories surface

A dedicated view (⌘3), reachable from the sidebar and the command palette.

### 10.1 Repositories list (main screen)

```
╔══════════════════════════════════════════════════════════════════════════════════╗
║  ⌘ forgehold ⌂ acme-team ▾        ⌕  search or press ⌘K            ⚡ 2   👤    ║
╠══════════════════════════════════════════════════════════════════════════════════╣
║                                                                                  ║
║  REPOSITORIES                                                   [ + Clone ]     ║
║                                                                                  ║
║  Filter: [ all ▾ ]  [ frontend ] [ infra ] [ legacy ]       ⌕ search repos      ║
║                                                                                  ║
║  ┌──────────────────────────────────────────────────────────────────────────┐   ║
║  │ ┃ forgehold                                             github · main     │   ║
║  │ ┃ Forgehold desktop app                                                       │   ║
║  │ ┃ ● clean  ↑ 0  ↓ 2     fetched 3m ago                                   │   ║
║  │ ┃ [ Open in Zed ]  [ Fetch ]  [ New branch ]   ⚙ Work with Claude       │   ║
║  └──────────────────────────────────────────────────────────────────────────┘   ║
║                                                                                  ║
║  ┌──────────────────────────────────────────────────────────────────────────┐   ║
║  │ ┃ acme-api                                      github · feat/sso-prov   │   ║
║  │ ┃ Billing & auth API                                                      │   ║
║  │ ┃ ◐ dirty (4 files)  ↑ 1  ↓ 0     fetched 12m ago                        │   ║
║  │ ┃ [ Open in Zed ]  [ Review changes ]  [ Stash ]   ⚙ Work with Claude   │   ║
║  └──────────────────────────────────────────────────────────────────────────┘   ║
║                                                                                  ║
║  ┌──────────────────────────────────────────────────────────────────────────┐   ║
║  │ ┃ acme-ui-kit                                   github · main             │   ║
║  │ ┃ Shared design system                                                    │   ║
║  │ ┃ ● clean  ↑ 0  ↓ 0     fetched 1h ago                                   │   ║
║  │ ┃ [ Open in Zed ]  [ Fetch ]  [ New branch ]   ⚙ Work with Claude       │   ║
║  └──────────────────────────────────────────────────────────────────────────┘   ║
║                                                                                  ║
╚══════════════════════════════════════════════════════════════════════════════════╝
```

**Status indicator (dot to the left of the text):**
- `●` green — clean, synced
- `●` amber — ahead/behind
- `◐` amber — dirty (uncommitted changes)
- `◆` red — diverged or merge conflict
- `○` grey — not fetched recently

### 10.2 Clone dialog

```
╭────────────────── Clone repository ──────────────────╮
│                                                      │
│  From:                                               │
│   ◉ GitHub (linked: @nikk)                          │
│   ○ Custom URL                                       │
│                                                      │
│   ⌕ search your repos…          [ acme/forgehold ▾ ]   │
│                                                      │
│  To:                                                 │
│   ~/Repos/forgehold-clones/forgehold                  📁     │
│                                                      │
│  Branch:                                             │
│   main  ▾                                            │
│                                                      │
│  ☑  Fetch all branches after clone                   │
│  ☐  Open in Zed after clone                          │
│                                                      │
│                          [ Cancel ]  [ Clone ]       │
╰──────────────────────────────────────────────────────╯
```

### 10.3 Repo detail view

Clicking a repo row opens an inline drawer or the detail view:

```
╔══════════════════════════════════════════════════════════════════════════════════╗
║  ← Repositories                                                                  ║
║                                                                                  ║
║  ┃ forgehold                                                                     ║
║  ┃ ~/Repos/pers/forgehold                                                        ║
║  ┃ github · acme/forgehold                                                       ║
║                                                                                  ║
║  [ Open in Zed ]   [ Fetch ]   [ New branch ]   [ Edit rules ]                  ║
║                                                                                  ║
║  ─── BRANCHES ───────────────────────────────────────────────────────────────    ║
║                                                                                  ║
║   ● main                                                     synced              ║
║   ○ feat/auth-retry-loop         (run-abc · claude worktree)                    ║
║   ○ feat/drop-zone-polish        last commit 2d ago                             ║
║   ○ chore/bump-deps              merged, safe to delete                          ║
║                                                                                  ║
║  ─── WORKTREES (2) ─────────────────────────────────────────────────────────    ║
║                                                                                  ║
║   run-abc · feat/auth-retry-loop · 00:42 running                                 ║
║   run-xyz · feat/drop-zone-polish · finished 1h ago                              ║
║                                                                                  ║
║  ─── RECENT RUNS (5) ───────────────────────────────────────────────────────    ║
║                                                                                  ║
║   PROJ-1234 → diff.patch   ✓ succeeded, 3m 12s                                   ║
║   PROJ-1199 → failed       ✗ pnpm typecheck failed                               ║
║   ...                                                                            ║
║                                                                                  ║
║  ─── RULES ─────────────────────────────────────────────────────────────────    ║
║                                                                                  ║
║   Repo rules: .forgehold/rules.md  (2 folder scopes defined)        [ Edit ]        ║
║                                                                                  ║
╚══════════════════════════════════════════════════════════════════════════════════╝
```

### 10.4 "Work with Claude" quick start

Clicking `⚙ Work with Claude` on a repo card opens a mini dialog without
requiring a ticket:

```
╭────────────── Work on forgehold with Claude ──────────────╮
│                                                        │
│  Describe the task:                                    │
│  ┌──────────────────────────────────────────────────┐ │
│  │ refactor auth retry loop, add exponential        │ │
│  │ backoff with max 5 attempts_                     │ │
│  └──────────────────────────────────────────────────┘ │
│                                                        │
│  Branch:  feat/refactor-auth-retry  ✎                 │
│  (generated from rules template)                      │
│                                                        │
│  Effective rules: 4 global · 7 repo · 3 folder  [▾]   │
│                                                        │
│  + Add rule for this run:                              │
│  ┌──────────────────────────────────────────────────┐ │
│  │                                                  │ │
│  └──────────────────────────────────────────────────┘ │
│                                                        │
│                   [ Cancel ]   [ ⚙  Start run ]       │
╰────────────────────────────────────────────────────────╯
```

---

## 11. Rules editor

### 11.1 Settings → Rules

```
╔══════════════════════════════════════════════════════════════════════════════════╗
║  Settings › Rules                                                                ║
╠══════════════════════════════════════════════════════════════════════════════════╣
║                                                                                  ║
║  Scope:  [ ● Global ] [ Repo: forgehold ▾ ] [ Folder: /apps/desktop/src/lib/ui ▾ ]  ║
║                                                                                  ║
║  ┌─────────────────────────────────┐   ┌─────────────────────────────────┐     ║
║  │ # TypeScript                    │   │ PREVIEW                         │     ║
║  │ - strict mode everywhere        │   │                                 │     ║
║  │ - no `any`, use `unknown`       │   │ Test case:                      │     ║
║  │                                 │   │ [ PROJ-1234 in /api ▾ ]         │     ║
║  │ # Commits                       │   │                                 │     ║
║  │ - Conventional Commits          │   │ --- Effective rules ---         │     ║
║  │                                 │   │                                 │     ║
║  │ @scope folder:/api              │   │ From your workspace:            │     ║
║  │ ## API layer                    │   │ • strict mode everywhere        │     ║
║  │ - all handlers return Result    │   │ • no any, use unknown           │     ║
║  │ - no direct DB access           │   │ • Conventional Commits          │     ║
║  │                                 │   │                                 │     ║
║  │                                 │   │ From repo forgehold:            │     ║
║  │                                 │   │ • Branch: feat/PROJ-{slug}      │     ║
║  │                                 │   │                                 │     ║
║  │                                 │   │ From folder /api:               │     ║
║  │                                 │   │ • Handlers return Result        │     ║
║  │                                 │   │ • No direct DB access           │     ║
║  │                                 │   │                                 │     ║
║  │                                 │   │ Branch will be named:           │     ║
║  │                                 │   │ feat/PROJ-1234-fix-auth-retry   │     ║
║  └─────────────────────────────────┘   └─────────────────────────────────┘     ║
║                                                                                  ║
║  [ Save ]    [ Save & commit to .forgehold/rules.md ]       Last saved: 2m ago     ║
║                                                                                  ║
╚══════════════════════════════════════════════════════════════════════════════════╝
```

Key points:
- Scope selector at the top, behaves like tabs.
- Split editor/preview: the preview shows the **final prompt** Claude
  will receive.
- Test case: pick a hypothetical object + folder, see what applies.
- Repo rules get a "Save & commit" button (auto `git add` + commit to
  `.forgehold/rules.md`).

### 11.2 Inline run override

See §4.1 — the Claude Code drop zone exposes an inline "Add rule for
this run" field.

### 11.3 Rules indicator on the drop zone

A chip under the action name shows how many rules will apply:

```
╔═══════════════════════════╗
║                           ║
║    ⚙  Claude Code         ║
║                           ║
║    Rules: 4·7·3  [▾]      ║
║                           ║
╚═══════════════════════════╝
```

Hover on `4·7·3` shows a tooltip with the breakdown (global / repo /
folder). Click expands the full preview.

---

## 12. Additional shortcuts

On top of §5.3:

```
⌘3        Repositories view
⌘⇧W       Work with Claude (quick ad-hoc run, pick the repo)
⌘⇧E       Edit rules for the currently selected scope
⌘L        Clone repository (open clone dialog)
⌘B        Switch branch (active repo)
```
