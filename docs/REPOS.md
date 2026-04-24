# Forgehold — Repositories

**Version:** 0.1 (draft)
**Last updated:** 2026-04-22

> How Forgehold manages local git repositories and ties them to the Claude
> agent and the GitHub Source.

---

## 1. Why Repository is a separate entity

The GitHub **Source** (see SPEC.md §2.1) is a connector to the GitHub API:
it reads issues, PRs, and comments. It *does not* touch local code.

**Repository** is a clone of a git repo on disk that Forgehold tracks — it
knows how to clone, update, branch, and hand the repo to the Claude
agent. A Repository can be linked to a GitHub Source (so Forgehold sees its
issues), or stand on its own (e.g. a local or GitLab repo we don't have
a Source for yet).

This split buys us:
- You can clone a private repo even if the GitHub Source is configured
  for a different organization.
- You can work in a repository that has no remote service at all (a
  local prototype).
- Claude runs receive an explicit target: "work in this repo", no guessing.

---

## 2. Entity schema

```rust
struct Repository {
    id: Uuid,
    workspace_id: Uuid,
    name: String,                    // "forgehold" or "acme/forgehold"
    remote_url: Option<String>,      // git@github.com:acme/forgehold.git
    local_path: String,              // absolute path on disk
    default_branch: String,          // from git, usually "main"
    linked_source_id: Option<Uuid>,  // optional link to a GitHub Source
    rules_path: Option<String>,      // typically ".forgehold/rules.md"
    tags: Vec<String>,               // filtering: "frontend", "infra"
    last_fetched_at: Option<Timestamp>,
    created_at: Timestamp,
}

struct RepoStatus {                  // not persisted, computed on-demand
    current_branch: String,
    head_sha: String,
    ahead: u32,                      // commits ahead of upstream
    behind: u32,                     // commits behind upstream
    dirty: bool,                     // uncommitted changes present
    untracked: u32,
    stashes: u32,
    sync_state: SyncState,
}

enum SyncState { Clean, Dirty, Ahead, Behind, Diverged, NoUpstream }
```

### SQL schema

```sql
CREATE TABLE repository (
  id                TEXT PRIMARY KEY,
  workspace_id      TEXT NOT NULL REFERENCES workspace(id),
  name              TEXT NOT NULL,
  remote_url        TEXT,
  local_path        TEXT NOT NULL UNIQUE,
  default_branch    TEXT NOT NULL,
  linked_source_id  TEXT REFERENCES source(id),
  rules_path        TEXT,
  tags              JSON NOT NULL DEFAULT '[]',
  last_fetched_at   INTEGER,
  created_at        INTEGER NOT NULL
);

CREATE INDEX idx_repo_workspace ON repository(workspace_id);
CREATE INDEX idx_repo_linked_source ON repository(linked_source_id);
```

RepoStatus isn't persisted — it's computed on the fly through `git` and
cached in memory with a 10-second TTL.

---

## 3. Lifecycle operations

All operations are *actions* in the SPEC.md sense, so they're available
from the UI, from workflows, and from the command palette.

### 3.1 Clone

```
Action: git.clone
Input:  { url: String, target_dir: String, name?: String }
Output: Repository
```

UX:
- A "+ Clone" button in the Repositories sidebar.
- A dialog with a URL field and a "My GitHub repos" dropdown (populated
  from the linked GitHub Source via API).
- `target_dir` auto-completes from settings: "where to store repos"
  (default: `~/Repos/forgehold-clones/`).
- Clone progress streams to the UI (`git clone --progress`).

### 3.2 Fetch / Pull

```
Action: git.fetch          # no merge
Action: git.pull           # fast-forward, fails on conflicts
Input:  { repo_id: Uuid }
Output: RepoStatus
```

UI: a "Sync" button on the repo card. Background: a periodic fetch for
open repos every 5 minutes while the user is in the app.

### 3.3 Branch management

```
Action: git.create_branch
Input:  { repo_id, name, from_branch?, checkout?: bool }
Output: Branch

Action: git.switch_branch
Input:  { repo_id, branch }
Output: RepoStatus

Action: git.list_branches
Input:  { repo_id, include_remote?: bool }
Output: Vec<BranchInfo>
```

Important: **Forgehold never runs `git checkout` when there are uncommitted
changes without explicit user confirmation.** This rule applies to every
destructive git operation.

### 3.4 Worktrees (central to Claude runs)

```
Action: git.create_worktree
Input:  { repo_id, branch, path?: String }
Output: Worktree { path: String, branch: String }

Action: git.remove_worktree
Input:  { worktree_path: String, force?: bool }
Output: void
```

Every Claude run operates in its **own worktree**, not the main working
directory. This is critical:
- The user keeps working in Zed in parallel with the agent.
- Changes are isolated: if a run goes sideways, the worktree is thrown
  away without affecting the main tree.
- It's easy to diff the result against main.

Worktrees live in `~/Library/Application Support/Forgehold/worktrees/<run_id>/`
and are auto-removed 24 hours after the run finishes (unless the user
pins the artifact).

### 3.5 Open in editor

```
Action: repo.open_in_editor
Input:  { repo_id, editor?: "zed" | "vscode" | "cursor" | "system" }
Output: void
```

Spawns `zed <path>` or `code <path>`. The default editor is read from
Forgehold settings (or the `$EDITOR` env var).

---

## 4. Integration with Claude runs

When the user drops a ticket onto Claude Code, the system has to know
**where to work**. Target repo resolution:

1. Explicit user choice in the drop zone (chip: "work in: acme/forgehold").
2. A default repo on the Source: if the ticket is from a Jira source, we
   look up its `default_repo_id` (a setting).
3. Ticket metadata: if the description references `acme/forgehold`, we try
   to auto-resolve.
4. Prompt the user: if nothing matched, ask explicitly.

Before the run starts:
- Create a worktree from `default_branch` (or from the branch specified
  in rules — see RULES.md §3.3).
- Generate the new branch name from the rules (typically
  `feat/PROJ-1234-description`).
- Read rules (global + repo + folder) and inject them as a prompt prefix.
- Spawn `claude -p` with the working directory set to the worktree path
  and `--add-dir` for reference-only access to the main repo (when
  needed).

---

## 5. Repository actions available in a workflow

| Action                   | Typical use                                        |
|--------------------------|----------------------------------------------------|
| `git.clone`              | onboarding, first time                             |
| `git.fetch`              | before a claude run — get a fresh default_branch   |
| `git.create_worktree`    | every claude run (automatic)                       |
| `git.commit`             | final workflow step ("claude → commit → PR")       |
| `git.push`               | after commit                                       |
| `github.create_pr`       | create a PR from an artifact diff (via GitHub source) |
| `repo.open_in_editor`    | final step: open the result in Zed                 |

**Note:** Forgehold never commits or pushes without explicit user consent
(the "auto-commit and push" checkbox in workflow settings is **off by
default**).

---

## 6. Multi-repo workspaces

A workspace can hold many repos (typical: a monorepo plus a couple of
standalone services). To keep things tidy:

- **Tags** on a repo: "frontend", "backend", "infra" — used to filter
  in the UI.
- **Default repo** per Source: when a workflow is created from a Jira
  ticket, the source's default repo is used automatically.
- **"Recent" repos** are tracked per-user for a quick-switch in the UI.

### Multi-root for a Claude run

Sometimes a ticket requires changes across multiple repos (e.g.
frontend + backend). MVP **does not** support this explicitly — a run
happens in one repo. Workaround: the user starts two runs manually.
Post-MVP we're considering "linked runs" with shared context.

---

## 7. Storage & configuration

### 7.1 Global settings

```toml
# ~/Library/Application Support/Forgehold/settings.toml
[repos]
default_clone_dir = "~/Repos/forgehold-clones"
default_editor = "zed"                   # or "code", "cursor", "system"
auto_fetch_interval_seconds = 300        # 5 min
worktree_dir = "~/Library/Application Support/Forgehold/worktrees"
worktree_ttl_hours = 24
```

### 7.2 Per-repo overrides

In `.forgehold/repo.toml` inside the repository:

```toml
[repo]
display_name = "Forgehold Desktop"
default_branch = "main"                  # overrides the git default
claude_worktree_from = "main"            # base branch for run worktrees
auto_commit = false                      # safe by default
```

This file is optional; everything works without it.

---

## 8. UI (brief reference; full detail in UI.md §10)

- **Sidebar item "Repositories"** with tag filters and search.
- **Repo row:** name, current branch, status chip (clean/dirty/ahead/
  behind), last fetch time, quick-action buttons.
- **Repo detail view:** branches (local + remote), recent commits,
  active worktrees, recent runs on this repo.
- **Clone dialog:** URL or a picker from GitHub repos, target folder,
  optional first pull.
- **"Work here with Claude" chip** on every repo card — a fast way to
  start an ad-hoc run without a ticket.

---

## 9. Security considerations

- **Never run `git clone` with an arbitrary URL without user
  confirmation.** SSH URLs especially — potential abuse through config
  hooks.
- **Sandbox the worktree path** strictly within the app data dir,
  verified with `canonicalize()` + prefix check.
- **Git hooks in a cloned repo** may be malicious. On first clone, warn
  if `.git/hooks/` contains non-default files.
- **SSH keys:** never copy or touch them; rely on the system
  `ssh-agent`.

---

## 10. Open items

- [ ] Submodules — support in MVP or not? Claude in a worktree with
  submodules can produce surprises.
- [ ] LFS — likely out of scope for MVP, but `git clone` must not fail
  on LFS repos (just skip fetching files).
- [ ] Shallow clones for large repos? Add an option in the clone dialog.
- [ ] GitHub CLI (`gh`) as a fallback for operations libgit2 doesn't
  handle well (PR creation). Worth considering.
