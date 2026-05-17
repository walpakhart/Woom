# Woom-bundled MCP tools (overview)

Five sidecars, each scoped to one domain. Tool schemas are already in context — this is an index for quick "does X exist" lookups.

## `app` — Woom UI driver

Surface for navigating + mutating Woom's own UI on behalf of the user.

- `mcp__app__switch_view` — change top-level solo (jira/github/sentry/claude/cursor/editor/canvas/terminal/home/connections/settings)
- `mcp__app__focus_solo` — bring rail attention to a kind
- `mcp__app__set_editor_repo_path` / `mcp__app__set_agent_cwd` — change folders bound to editor / agent
- `mcp__app__add_app_instance` — spawn new editor/canvas/terminal instance
- `mcp__app__list_instances` — enumerate every open instance
- `mcp__app__open_github_pr` / `_issue` / `mcp__app__open_jira_issue` / `mcp__app__open_sentry_issue` / `_event` — slide-over detail panes
- `mcp__app__open_github_repo` — github solo + repo + section (code/pulls/issues/actions/releases)
- `mcp__app__open_jira_tab` / `mcp__app__open_sentry_tab` — filtered list views
- `mcp__app__propose_bash` / `mcp__app__propose_switch_cwd` — approval-blocked actions
- `mcp__app__ensure_terminal` / `mcp__app__terminal_run` / `_write` / `_buffer` / `_list` — drive PTY terminals
- `mcp__app__bg_spawn` / `_list` / `_logs` / `_kill` / `_wait_line` / `_stdin` — long-running background tasks (Preview pane)
- `mcp__app__canvas_*` — add/update/delete/group/align/arrange shapes + edges + viewport on a canvas

## `github` — GitHub read + write

PRs, issues, repos, branches, commits, checks, comments, releases.

- Read: `get_pr` / `get_pr_diff` / `get_pr_files` / `get_pr_comments` / `list_check_runs` / `list_commits` / `list_releases` / `list_workflow_runs` / `get_file` / `list_tree` / `get_readme` / `search_prs` / `search_issues` / `list_repos`
- Write (immediate): `add_comment` / `submit_review` / `merge_pr` / `edit_pr` / `request_reviewers` / `remove_reviewers` / `add_labels` / `remove_labels` / `add_assignees` / `remove_assignees` / `set_pr_draft` / `set_pr_state` / `rerun_workflow` / `cancel_workflow`
- Approval-blocked (`propose_*`): `propose_commit` / `propose_pr`

## `jira` — Jira (Atlassian Cloud)

- Read: `get_issue` / `search` (JQL) / `list_projects` / `list_assignable_users` / `list_sprints` / `list_boards` / `list_statuses` / `list_issue_types` / `list_worklogs`
- Write: `add_comment` / `transition_issue` / `create_issue` / `update_issue` / `set_priority` / `add_worklog`

## `sentry` — Sentry

- Read: `get_issue` / `get_event` / `get_issue_tags` / `list_events` / `search_issues` / `list_projects` / `list_releases`
- Write: `update_issue` / `add_comment`

## `memory` — long-term store (SQLite + FTS5)

- `memory_save` / `memory_search` / `memory_list` / `memory_get` / `memory_update` / `memory_delete`
- Kinds: `user` / `feedback` / `project` / `reference` / `note`. First-200-lines of `user` + `feedback` auto-injected at session start.

## Out-of-band features (not sidecar tools)

These are Woom-side features the agent sees as system-prompt context, not callable tools:

- **Hooks** — user shell scripts run on UserPromptSubmit / Stop / SessionStart (see Settings → Hooks)
- **Skills** — user-defined `/<name>` slash commands at `~/.claude/skills/<name>/SKILL.md`, with `$ARGUMENTS` + `` !`<cmd>` `` shell injection
- **CLAUDE.md** — walked from cwd to repo root, prepended to system prompt each turn
- **Plan mode** — `⇧⇥` in composer flips the session to read-only; the agent reads instructions in its system prompt
- **`/loop <duration> <prompt>`** — recurring user message on a cadence (7-day expiry)
- **Statusline** — user shell script piped session JSON, output rendered below composer
