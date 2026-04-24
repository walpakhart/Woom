# Forgehold — Rules System

**Version:** 0.1 (draft)
**Last updated:** 2026-04-22

> How to say "write code like this", "name branches like this", "this
> folder has its own conventions" — and have the Claude agent respect
> all of it. Scoped visibility, composition, a clear UI.

---

## 1. Why this exists

Every repo and every team has unwritten rules: how to name branches,
how to phrase commits, the style inside a specific folder, which tests
are mandatory. Today this lives in people's heads, READMEs, or
CONTRIBUTING.md — and the Claude agent has to be re-briefed every time.

Forgehold Rules is a declarative way to describe those rules once and have
them injected into every Claude run automatically.

---

## 2. Scopes

Four levels, from broad to narrow:

| Scope             | Where it lives                    | Applies to                                    |
|-------------------|-----------------------------------|-----------------------------------------------|
| `global`          | Forgehold app data (workspace-wide)   | All runs in the workspace                     |
| `repo`            | `.forgehold/rules.md` in the repo     | All runs in this repository                   |
| `folder:<path>`   | section in the repo `rules.md`    | Runs touching files in this folder            |
| `run`             | ephemeral, inline in the drop zone| The current run only                          |

**Global** — team standards ("we use TypeScript", "commits are
Conventional Commits").

**Repo** — project specifics ("frontend is Svelte", "we follow SemVer").

**Folder** — local conventions ("in /api/ — async/await only", "in
/legacy/ — don't touch the architecture").

**Run** — one-off directives ("no tests this time, urgent hotfix").

---

## 3. rules.md structure

Markdown with a YAML frontmatter for metadata and `@scope` directives
for sections.

### 3.1 Minimal example

```markdown
---
name: Forgehold repo rules
version: 1
---

# Branch naming
- Feature branches: `feat/PROJ-XXXX-kebab-case`
- Fixes: `fix/PROJ-XXXX-kebab-case`
- Chores: `chore/description`

# Commits
- Conventional Commits: `feat: …`, `fix: …`, `chore: …`, `docs: …`
- When a ticket is known, append `(PROJ-XXXX)` to the title

# Pull requests
- Title = conventional commit (drop the `feat:` prefix if generic)
- Body: a one- or two-line summary + a test-plan checklist
```

### 3.2 With folder-scoped sections

```markdown
---
name: Forgehold repo rules
version: 1
---

# Branch naming
- ...

@scope folder:/apps/desktop/src/lib/ui

## UI components
- Use Svelte 5 runes, not the old `$:` syntax
- Styling — Tailwind + design tokens from `@forgehold/ui/tokens`
- Motion components must import `motion-presets` (no hardcoded values)

@scope folder:/crates/forge-core

## Core domain
- No dependencies on Tauri or UI — pure domain
- Errors through `thiserror`, not `anyhow`
- All public functions require doc comments

@scope folder:/mcp-servers

## MCP servers
- Each server is a standalone binary, not linked with forge-core
- Logging goes to stderr (stdio is reserved for MCP protocol)
- MCP protocol version pinned in Cargo.toml
```

### 3.3 Workflow-specific directives

The frontmatter can hold Claude-run parameters:

```yaml
---
name: Forgehold repo rules
version: 1
claude:
  branch_from: main              # which branch to base the worktree on
  branch_template: "feat/{ticket.key}-{ticket.slug}"
  commit_template: "{type}: {summary}\n\nRefs: {ticket.url}"
  pre_run_checks:
    - "pnpm install"             # required before the run
  post_run_checks:
    - "pnpm typecheck"
    - "pnpm test --run"
  forbidden_paths:
    - "**/secrets/*"
    - ".env*"
---
```

forge-runtime reads these before launching Claude and applies them
automatically. A `branch_template` containing `{ticket.key}` triggers
substitution from the dropped object.

---

## 4. Resolution (how rules compose)

When a Claude run starts:

```
1. Load global rules (from workspace app data).
2. Load repo rules (from .forgehold/rules.md).
3. Determine the run's "working area":
   - If a target folder is explicit — use it.
   - If not — keep all folder rules potentially applicable, and the
     agent sees them as "if you touch /api, these apply".
4. Load folder rules relevant to the working area.
5. Add run rules (inline from the user).
6. Compose the final prompt prefix (see §5).
```

Conflicts across levels:
- A narrower scope **overrides** a broader one on the same topic.
- Example: global says "commits in English", folder `/legacy` says
  "commits in Russian" — in /legacy, folder wins.
- Explicit conflicts are annotated in the prompt: "Note: folder rules
  override workspace rule X for files in /legacy".

---

## 5. How rules reach Claude

Forgehold assembles a system prompt prefix:

```
You are working on {repo.name} in a worktree at {worktree.path}.
The default branch is {repo.default_branch}.

## Rules from your workspace
{global rules as a bullet list}

## Rules from this repository
{repo rules}

## Rules for the folder you'll be working in
{folder rules relevant to the task}

## Special instructions for this run
{run rules}

## Automated policies
- Branch name must match: `{resolved branch_template}`
- After you finish, run: {post_run_checks}
- Do NOT edit these paths: {forbidden_paths}

---

Now, here is the task:

{object.title}

{object.body}
```

This prefix goes as the system instruction for `claude -p`, followed by
the task itself.

### 5.1 Interaction with native CLAUDE.md

Claude already reads `CLAUDE.md` and `.claude/CLAUDE.md` in the repo.
Forgehold Rules **does not** break that — we don't modify CLAUDE.md.

Behavior:
- If the repo has a `CLAUDE.md`, Claude reads it natively.
- If the user has `.forgehold/rules.md`, Forgehold injects it as an additional
  system prompt.
- Both work together: CLAUDE.md for baseline constants, Forgehold Rules for
  workflow specifics (templates, policies, dynamic data like ticket
  info).

**UI recommendation:** when a user creates repo rules and there's no
CLAUDE.md yet, offer to export the baseline to CLAUDE.md (so things
work outside Forgehold too, in plain Claude Code).

---

## 6. UI (brief reference; full detail in UI.md §11)

### 6.1 Rules editor

- Reachable from Settings → Rules and from the Inspector when a
  Repository is selected.
- Split view: markdown editor on the left, live preview of "effective
  rules for test case X" on the right (pick a test case, see the final
  prompt prefix).
- Scope selector: global / repo / folder (with a folder picker for the
  repo).

### 6.2 Per-run override

In the Workbench, when you drop an object on the Claude drop zone, an
inline field appears:

```
┌─────────────────────────────────────────────┐
│  ⚙  Claude Code                             │
│                                             │
│  Rules: 4 global · 7 repo · 3 folder  [▾] │
│                                             │
│  + Add rule for this run:                   │
│  ┌─────────────────────────────────────┐   │
│  │ use tabs for indentation here, the   │   │
│  │ legacy style_                        │   │
│  └─────────────────────────────────────┘   │
│                                             │
└─────────────────────────────────────────────┘
```

Clicking `[▾]` expands effective rules — the user sees exactly what
the agent will receive.

### 6.3 Preview resolved rules

A "Preview" button in the rules editor shows the final prompt as the
agent will see it, for a hypothetical scenario (object + folder). This
solves the classic "which rules actually apply?" problem.

---

## 7. Versioning & team sync

### 7.1 Repo rules are committed to the repo

`.forgehold/rules.md` is a normal file in the repo. Reviewed via git, PRs,
blame. The team agrees on rules the same way they agree on code.

### 7.2 Global rules are local for now

In MVP, global rules live on the user's machine. In post-MVP (v0.3)
they'll sync through the backend as a workspace-shared resource.

### 7.3 Rule migration

The `version: 1` field in the frontmatter guards against future
breaking schema changes. If Forgehold sees `version: 2` and doesn't support
it, it refuses to apply and asks the user to update the app.

---

## 8. Worked example

Say we have:

**`~/Library/Application Support/Forgehold/rules.global.md`** (workspace):
```markdown
---
name: Acme Team standards
---
# All repos
- TypeScript strict mode everywhere
- No `any`, use `unknown` + narrowing
- Tests obligatory for new exported functions
```

**`<repo>/.forgehold/rules.md`** (repo-level):
```markdown
---
name: Forgehold desktop rules
claude:
  branch_template: "feat/{ticket.key}-{ticket.slug}"
  post_run_checks: ["pnpm typecheck"]
---
# Branch and commit
- Conventional Commits
- PR title = commit title

@scope folder:/apps/desktop/src/lib/ui
## UI components
- Svelte 5 runes
- No inline styles; use Tailwind
```

**The user drops** ticket `PROJ-1234 "Add retry button to auth dialog"`
on Claude Code. Run rules are empty.

**The effective prompt prefix Claude receives:**

```
You are working on forgehold in worktree /Users/nik/.../worktrees/run-abc.
Default branch: main.

## Rules from your workspace
- TypeScript strict mode everywhere
- No `any`, use `unknown` + narrowing
- Tests obligatory for new exported functions

## Rules from this repository
- Conventional Commits
- PR title = commit title

## Rules for the folder you'll be working in (auth dialog is in /apps/desktop/src/lib/ui)
- Svelte 5 runes
- No inline styles; use Tailwind

## Automated policies
- Branch name must match: `feat/PROJ-1234-add-retry-button-to-auth-dialog`
- After finish, run: pnpm typecheck

---

Now, here is the task:

PROJ-1234: Add retry button to auth dialog

[ticket body]
```

The agent runs in a worktree on branch
`feat/PROJ-1234-add-retry-button-to-auth-dialog`. After finishing,
Forgehold invokes `pnpm typecheck`; if it passes, the artifact is marked
green; if it fails, red with the error output.

---

## 9. Open items

- [ ] Conditional rules (`if touching /api then …`) — not in MVP;
  considered for v0.2.
- [ ] User-defined variables in templates (`{team.slug}`, `{date.iso}`)?
  Useful, but complexity grows. For now, only `{ticket.*}`, `{repo.*}`,
  `{user.*}`.
- [ ] How shared repo rules interact with forks. Forks should likely
  inherit, with the ability to override.
- [ ] "Linting" rules — can we statically validate that rules don't
  contradict each other?
