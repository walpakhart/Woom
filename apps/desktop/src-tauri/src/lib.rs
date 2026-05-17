mod action_ipc;
mod agent;
mod bg_tasks;
mod biometry;
mod claude;
mod claude_mcp;
mod claude_quota;
mod claudemd;
mod crash_reporting;
mod cursor;
mod cursor_mcp;
mod fs;
mod git;
mod github;
mod hooks;
mod jira;
mod keychain;
mod library;
mod memory_local;
mod sdd;
mod sentry;
mod skills;
mod statusline;
mod terminal;
mod terminal_bridge;
mod watch;
mod worktree;

use agent::{AgentAskResult, AgentKind, AgentStatus};
use claude::{ClaudeStatus, Runners, WarmPool};
use fs::{BashResult, DirEntry};
use git::{Branch, CommitEntry as GitCommitEntry, GitStatus, RepoInfo};
use tauri::State;
use github::{
    ChangedFile, CheckRun, Comment, CommitDetail, CommitEntry, CompareResult, FileBlob,
    GithubUser, InboxItem, PrDetail, Release, RepoBranch, RepoCommit, RepoReadme, Repository,
    Review, ReviewComment, TreeEntry, WorkflowRun,
};
use jira::{
    JiraBoard, JiraComment, JiraCredentials, JiraDetail, JiraIssueType, JiraItem, JiraProject,
    JiraSprint, JiraStatus as JiraWorkflowStatus, JiraUser, JiraUserSummary, JiraWorklog,
};
use sentry::{
    SentryCredentials, SentryEnvironment, SentryEvent, SentryEventDetail, SentryIssue,
    SentryProject, SentryUser,
};
use serde::Serialize;
use watch::WatcherState;
use worktree::{Worktree, WorktreeChangedFile};

const GITHUB_KEY: &str = "github";
const JIRA_KEY: &str = "jira";
const SENTRY_KEY: &str = "sentry";

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectionStatus {
    Disconnected,
    /// `rate_limit` is `None` when the upstream response didn't include
    /// `x-ratelimit-*` (rare — every authenticated GitHub call sets
    /// them, but some corporate proxies strip headers).
    Connected {
        user: GithubUser,
        #[serde(skip_serializing_if = "Option::is_none")]
        rate_limit: Option<github::RateLimit>,
    },
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JiraStatus {
    Disconnected,
    Connected { user: JiraUser },
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SentryConnectionStatus {
    Disconnected,
    Connected { user: SentryUser },
}

/// When Woom is launched from Finder/Dock (not a terminal) the process
/// inherits macOS's minimal PATH (`/usr/bin:/bin:/usr/sbin:/sbin`) — the
/// user's `~/.zshrc`/`.bash_profile` never runs. Tools like `claude`,
/// `cursor-agent`, and anything installed via Homebrew/nvm/bun/volta live
/// outside that PATH, so the app can't find them.
///
/// Fix: spawn the user's login shell once at startup, ask it to print its
/// fully-resolved PATH, and overwrite our own. Every `std::env::var("PATH")`
/// read (including the `which()` lookups in `claude.rs` / `cursor.rs`) then
/// sees the right values, as do all spawned subprocesses.
fn hydrate_path_from_login_shell() {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let out = std::process::Command::new(&shell)
        .arg("-l")
        .arg("-c")
        .arg("printf %s \"$PATH\"")
        .output();
    if let Ok(out) = out {
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !path.is_empty() {
                // SAFETY: Rust 2024 marks `set_var` unsafe because mutating
                // process env is unsound when other threads may read it
                // concurrently. We're called once, synchronously, at the
                // top of `run()` — before Tauri spawns its event loop or
                // any tokio worker. No reader thread exists yet.
                unsafe { std::env::set_var("PATH", path) };
            }
        }
    }
}

/// In-app docs viewer (`docs/ROADMAP_1.0.md §1.10`).
///
/// Loads bundled Markdown specs from the .app's `Contents/Resources`
/// dir. The dev path falls back to the repo's `docs/` folder so
/// `pnpm tauri dev` works without re-bundling.
///
/// Returns an alphabetised list of `*.md` filenames (without
/// extension) so the frontend can render a picker. Hidden files are
/// skipped.
#[tauri::command]
fn list_bundled_docs(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let dir = bundled_docs_dir(&app)?;
    let mut names = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| {
        format!("read docs dir {}: {e}", dir.display())
    })?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.starts_with('.'))
            .map(String::from);
        if let Some(s) = stem {
            names.push(s);
        }
    }
    names.sort();
    Ok(names)
}

#[tauri::command]
fn read_bundled_doc(app: tauri::AppHandle, name: String) -> Result<String, String> {
    /* Hard sanitization: the doc name is a frontend-supplied string,
     * so prevent any path-traversal joys. Only accept `[A-Za-z0-9_]+`,
     * which matches every spec we ship. */
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(format!("invalid doc name: {name}"));
    }
    let dir = bundled_docs_dir(&app)?;
    let path = dir.join(format!("{name}.md"));
    /* Belt + suspenders: re-canonicalize and verify it's still
     * under `dir` after the join. Defensive against rare symlink-
     * resolution edge cases. */
    let canonical_dir = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
    let canonical_path =
        std::fs::canonicalize(&path).map_err(|e| format!("doc not found: {e}"))?;
    if !canonical_path.starts_with(&canonical_dir) {
        return Err(format!("doc {name} resolved outside docs dir"));
    }
    std::fs::read_to_string(&canonical_path)
        .map_err(|e| format!("read {}: {e}", canonical_path.display()))
}

/// Resolve where bundled docs live. In production this is the .app's
/// resource dir; in dev (`pnpm tauri dev`) the resource dir doesn't
/// exist, so we fall back to the repo's `docs/` two levels up from
/// the cwd (matches the workspace structure: `apps/desktop/`).
fn bundled_docs_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    if let Ok(resource_dir) = app.path().resource_dir() {
        let docs = resource_dir.join("docs");
        if docs.is_dir() {
            return Ok(docs);
        }
    }
    /* Dev fallback. Walk up from the executable until we find a
     * `docs/` sibling. Bounded to 8 hops so a misconfigured launch
     * doesn't spin. */
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cur = exe.parent().map(|p| p.to_path_buf());
    for _ in 0..8 {
        if let Some(p) = &cur {
            let candidate = p.join("docs");
            if candidate.is_dir() {
                return Ok(candidate);
            }
            cur = p.parent().map(|x| x.to_path_buf());
        } else {
            break;
        }
    }
    Err("docs directory not found (build with bundle resources or run from repo)".into())
}

/// Kill any Woom sidecar processes left running from a previous
/// session. See `run()` for why this matters — Cursor's MCP client
/// keeps sidecars alive across restarts and serves their (old) tool
/// schema until the process dies.
///
/// Uses `pkill -f` matching on the bundled binary names. Best-effort:
/// if pkill isn't available or no matches exist, exit is non-zero and
/// we just shrug. Runs synchronously before the Tauri event loop so
/// the cursor_mcp::sync() that follows writes config for fresh
/// processes.
fn kill_stale_sidecars() {
    /* `pkill -f` matches against the full command line, so paths like
       /Applications/Woom.app/Contents/MacOS/woom-app match
       cleanly. We pkill each sidecar binary by exact suffix name —
       avoids any regex-alternation portability questions and keeps
       the match deliberately scoped to our bundled binaries (never
       touches woom-desktop, which is the main process). */
    for name in ["woom-app", "woom-github", "woom-jira", "woom-sentry", "woom-memory"] {
        let _ = std::process::Command::new("pkill")
            .arg("-f")
            .arg(name)
            .output();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    hydrate_path_from_login_shell();
    /* Crash reporting (`docs/ROADMAP_1.0.md §1.3`). Opt-out file lives
     * under app-support — checked here before any potentially-
     * panicking code so a user who's flipped the toggle never gets
     * a panic-handler swap. The actual SDK init is currently a no-
     * op; the toggle still functions and the plumbing is in place
     * for the moment a Sentry DSN ships with the build. See
     * `crash_reporting::init_if_enabled` for the wiring. */
    crash_reporting::init_if_enabled();
    // Kill any stale Woom sidecars left behind by Cursor / Claude
    // from a previous Woom version. Cursor's MCP client spawns
    // sidecars on first handshake and keeps them alive across Cursor
    // restarts and Woom restarts — `ps aux | grep woom-app`
    // shows the original PID days later. After a DMG update, those
    // long-lived sidecars run the OLD binary's tool schema; the new
    // tools / aliases / batch endpoints simply aren't there for the
    // agent to call. Killing them here forces Cursor (and any
    // already-running Claude session) to spawn fresh ones from the
    // bundle we're about to register, which solves the "I just
    // updated Woom but Cursor still says missing field
    // from_shape_id" class of bugs.
    //
    // Single-user macOS app, so there's never a "kill the other
    // user's Woom sidecars" risk to worry about.
    kill_stale_sidecars();
    // Push the current set of Woom-owned MCP entries (jira / github /
    // sentry / memory / app) into `~/.cursor/mcp.json` once at startup.
    // Without this, an existing user wouldn't pick up new sidecars (most
    // notably `woom-app` for UI-navigation tools) until they touched
    // a connect/disconnect toggle. Best-effort — a failure here just leaves
    // the cursor mcp config a turn behind; no UX impact otherwise.
    let _ = cursor_mcp::sync();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        // Auto-updater plugin (`docs/ROADMAP_1.0.md §1.3`).
        // Reads the update endpoint + pubkey from `tauri.conf.json
        // > plugins > updater`. The pubkey there is a placeholder
        // until the release pipeline lands a real signing key (see
        // README → "Releasing 1.0"). The plugin itself is harmless to
        // ship without a key — frontend calls to `check()` simply
        // return `null` (no update available) when the manifest URL
        // is unreachable.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(claude::new_runners())
        .manage(claude::new_warm_pool())
        .manage(watch::new_state())
        .manage(terminal::TerminalRegistry::default())
        .manage(bg_tasks::BgRegistry::new())
        .manage(sdd::SddRegistry::new())
        .manage(action_ipc_state())
        .invoke_handler(tauri::generate_handler![
            github_connect_pat,
            github_status,
            github_disconnect,
            github_list_inbox,
            github_search_inbox,
            github_list_repos,
            github_list_repo_items,
            github_get_inbox_item,
            github_list_workflow_runs,
            github_rerun_workflow,
            github_cancel_workflow,
            github_list_tree,
            github_get_file_content,
            github_list_releases,
            github_list_repo_commits,
            github_list_repo_branches,
            github_get_readme,
            github_get_pr,
            github_list_pr_files,
            github_list_pr_commits,
            github_list_check_runs,
            github_list_pr_reviews,
            github_list_review_comments,
            github_list_comments,
            github_get_commit,
            github_add_comment,
            github_submit_review,
            github_set_state,
            github_merge_pr,
            github_edit_pr,
            github_request_reviewers,
            github_remove_reviewers,
            github_add_labels,
            github_remove_labels,
            github_add_assignees,
            github_remove_assignees,
            github_set_pr_draft,
            github_compare,
            github_create_pr,
            jira_connect,
            jira_status,
            jira_disconnect,
            sentry_connect,
            sentry_status,
            sentry_disconnect,
            sentry_list_issues,
            sentry_get_issue,
            sentry_list_events,
            sentry_list_projects,
            sentry_list_environments,
            sentry_get_event_detail,
            sentry_set_status,
            cursor_mcp_sync,
            jira_list_inbox,
            jira_list_inbox_for,
            jira_search,
            jira_list_projects,
            jira_list_boards,
            jira_list_sprints,
            jira_list_statuses,
            jira_list_issue_types,
            jira_create_issue,
            jira_search_users,
            jira_list_assignable_users,
            jira_get_issue_detail,
            jira_update_issue,
            jira_transition_issue,
            jira_set_assignee,
            jira_set_priority,
            jira_set_labels,
            jira_add_comment,
            jira_list_worklogs,
            jira_add_worklog,
            jira_delete_worklog,
            claude_status,
            claude_ask,
            claude_prewarm,
            claude_drop_prewarm,
            agent_compact_session,
            claude_plan_usage,
            claude_stop,
            agent_generate_commit_message,
            agent_status,
            library_list_installed,
            library_install_skill_git,
            library_install_skill_inline,
            library_install_anthropic_skill,
            library_install_skill_from_repo,
            library_plugin_install_anthropic,
            library_uninstall_skill,
            library_plugin_marketplace_add,
            library_plugin_install,
            library_plugin_uninstall,
            fs_read_file,
            fs_write_file,
            fs_write_bytes,
            revert_edit,
            revert_write,
            restore_deleted_file,
            redelete_file,
            app_data_dir,
            set_window_zoom,
            fs_list_dir,
            fs_walk_files,
            fs_search_text,
            fs_path_exists,
            fs_bash_run,
            git_status,
            git_check_ignore,
            git_ls_files,
            git_branches,
            git_current_branch,
            git_checkout,
            git_create_branch,
            git_fetch,
            git_stage,
            git_unstage,
            git_discard,
            git_commit,
            git_push,
            git_pull,
            git_log,
            git_repo_root,
            git_repo_info,
            git_diff,
            git_show,
            pre_write_baseline,
            git_commit_and_push,
            git_create_pr,
            git_gh_cli_available,
            pr_create_available,
            fs_watch_start,
            fs_watch_stop,
            worktree_create,
            worktree_remove,
            worktree_list,
            worktree_disk_usage,
            worktree_storage_dir,
            worktree_cleanup_orphans,
            worktree_diff,
            worktree_apply,
            biometric_unlock,
            crash_reporting::get_telemetry_opt_out,
            crash_reporting::set_telemetry_opt_out,
            list_bundled_docs,
            read_bundled_doc,
            fs_remove_file,
            fs_remove_dir,
            fs_rename,
            fs_reveal_in_finder,
            mcp_sidecar_health,
            memory_local::memory_save_local,
            memory_local::memory_search_local,
            memory_local::memory_stats_local,
            memory_local::memory_list_local,
            memory_local::memory_delete_local,
            memory_local::memory_session_counts_local,
            memory_local::memory_update_local,
            // Terminal app — PTY-backed shell per terminal instance.
            // Spawn returns a stable id; output streams over
            // `terminal:output:<id>` Tauri events; write/resize/kill
            // address by id. See `terminal.rs`.
            terminal::terminal_spawn,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_kill,
            // Background tasks — long-running processes the agent (or
            // user) spawns and wants to watch (dev servers, build
            // loops, test runners). See `bg_tasks.rs`.
            bg_tasks::bg_spawn,
            bg_tasks::bg_list,
            bg_tasks::bg_get,
            bg_tasks::bg_kill,
            bg_tasks::bg_send_stdin,
            bg_tasks::bg_logs,
            bg_tasks::bg_wait_line,
            bg_tasks::preview_open_window,
            // SDD (Spec-Driven Development) — orchestrated spec → plan
            // → phases workflow in a temp workspace under
            // `<app_data>/sdd-workspaces/<id>/`. See `sdd.rs`.
            sdd::sdd_start,
            sdd::sdd_hydrate,
            sdd::sdd_get,
            sdd::sdd_list,
            sdd::sdd_refresh,
            sdd::sdd_approve,
            sdd::sdd_pause,
            sdd::sdd_resume,
            sdd::sdd_stop,
            sdd::sdd_prompt,
            sdd::sdd_save_body,
            sdd::sdd_retry_phase,
            sdd::sdd_discard,
            // User-defined hooks — agent-lifecycle scripts. See
            // `hooks.rs` for the contract (stdin JSON / exit code /
            // stdout JSON). `hooks_run` is called from the frontend
            // at the SessionStart / UserPromptSubmit / Stop points
            // wired in `+page.svelte`.
            hooks::hooks_load_config,
            hooks::hooks_save_config,
            hooks::hooks_run,
            // Skills — user-defined slash commands with `!`-shell
            // injection (Claude Code parity §3). Discover walks
            // `~/.claude/skills` + `<cwd>/.claude/skills` upward.
            // Render expands `$ARGUMENTS` + ``!`<cmd>`` substitutions.
            skills::skills_discover,
            skills::skills_render,
            skills::skills_install_bundled_defaults,
            // Statusline — pipes session state JSON to a user shell
            // script and renders stdout in a thin strip below the
            // composer. Config at <app_data>/statusline.json.
            statusline::statusline_load_config,
            statusline::statusline_save_config,
            statusline::statusline_run,
            // CLAUDE.md auto-load — walks cwd up to root, prepends
            // ~/.claude/CLAUDE.md, strips HTML comments, expands
            // @path imports recursively (cap 5). Frontend caches
            // per-cwd and stamps the content into the agent's
            // system-prompt suffix on each turn.
            claudemd::claudemd_load,
            resolve_action_wait,
        ])
        .setup(|app| {
            // Hooks config — load at startup so the frontend's first
            // `hooks_load_config` call doesn't pay a disk read. Failure
            // modes (missing/malformed) fall through to an empty config
            // inside `HookState::new` (see hooks.rs comments).
            app.manage(hooks::HookState::new(app.handle()));
            app.manage(statusline::StatusLineState::new(app.handle()));
            // Bring up the action-IPC Unix socket — sidecars use it
            // to make `propose_*` MCP tools BLOCKING (they hold the
            // MCP response open until the user resolves the card).
            // Failure is non-fatal: the propose_* tools degrade to
            // returning an error string, the agent surfaces it, and
            // the user can still send manual messages.
            //
            // CRITICAL: must run inside an async-runtime task, NOT
            // synchronously in setup. `UnixListener::bind` and
            // `tokio::spawn` both require a Tokio runtime context;
            // calling them from setup-fn directly panics with "no
            // reactor running" → SIGABRT during
            // NSApplicationDidFinishLaunching → app crash on launch.
            use tauri::Manager;
            let ipc: tauri::State<'_, std::sync::Arc<action_ipc::ActionIpc>> = app.state();
            let ipc_handle = ipc.inner().clone();
            let ipc_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = action_ipc::start_ipc_server(ipc_handle, ipc_app) {
                    eprintln!("[woom] action IPC server failed to start: {e}");
                }
            });
            // Spawn the localhost terminal-MCP bridge. Failure is
            // non-fatal — desktop still works, agents just lose
            // `terminal.run_command` etc. Port goes to
            // `<app_data>/bridge.port` for sidecar discovery.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match terminal_bridge::start(handle.clone()).await {
                    Ok(port) => {
                        eprintln!(
                            "[woom] terminal MCP bridge listening on 127.0.0.1:{port}"
                        );
                    }
                    Err(e) => {
                        eprintln!("[woom] terminal bridge failed to start: {e}");
                    }
                }
            });
            // Background sweeper for the Claude warm pool — kills any
            // prewarmed CLI that's been parked longer than its TTL
            // (~150s). Without this, abandoned prewarms (user typed in
            // a chat then closed the tab without sending) would
            // accumulate over the app's lifetime, each holding open a
            // claude process + MCP sidecars.
            let warm_pool: tauri::State<'_, claude::WarmPool> = app.state();
            let warm_pool_handle = warm_pool.inner().clone();
            tauri::async_runtime::spawn(async move {
                // Tick every 30s — fine-grained enough that newly-stale
                // entries don't sit much past their TTL, infrequent
                // enough that the lock contention cost is negligible.
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
                ticker.tick().await; // skip the immediate first tick
                loop {
                    ticker.tick().await;
                    claude::evict_stale_warm(warm_pool_handle.clone()).await;
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Kill our sidecars when Woom quits. Tauri owns the
            // main `woom-desktop` process, but the `woom-app
            // / -github / -jira / -sentry / -memory` MCP sidecars are
            // spawned by Cursor (and Claude Code) on their first
            // handshake, NOT by us — they become Cursor's children, so
            // a normal cmd-Q on Woom leaves them happily running
            // for hours / days / until reboot, serving the OLD tool
            // schema to whatever agent connects to them next.
            //
            // `RunEvent::Exit` fires after the last window closes and
            // we're about to leave the event loop — best moment to
            // sweep them. We use the same `pkill -f` matching the
            // startup `kill_stale_sidecars` does, so the next launch
            // of Woom (or Cursor immediately reconnecting via
            // MCP) gets a clean slate.
            if let tauri::RunEvent::Exit = event {
                terminal_bridge::clear_port_file(app);
                // Best-effort cleanup of the action-IPC socket so the
                // next launch doesn't see a stale-looking inode. The
                // server itself also unlinks on bind, so this is just
                // tidiness — not a correctness requirement.
                use tauri::Manager;
                if let Some(ipc) = app.try_state::<std::sync::Arc<action_ipc::ActionIpc>>() {
                    let _ = std::fs::remove_file(ipc.inner().socket_path());
                }
                kill_stale_sidecars();
            }
        });
}

async fn token() -> Result<String, String> {
    keychain::get(GITHUB_KEY)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "GitHub is not connected".to_string())
}

/// Build the IPC server state with a per-process socket path. The path
/// formula lives in `action_ipc::current_socket_path` so the listener
/// (here) and the env value pushed to `~/.cursor/mcp.json` (see
/// `cursor_mcp::sync`) stay in lock-step.
fn action_ipc_state() -> std::sync::Arc<action_ipc::ActionIpc> {
    std::sync::Arc::new(action_ipc::ActionIpc::new(action_ipc::current_socket_path()))
}

/// Resolve a pending approval-card wait. The frontend calls this
/// after running an action card to completion (success or failure).
/// `summary` is what the agent sees as the MCP tool result text, so
/// it should be terse but informative — exit codes, commit hashes,
/// PR urls, error stderr tails. The sidecar's `propose_*` MCP call
/// returns this verbatim and the agent's tool_result lands with it
/// IN THE SAME TURN.
#[tauri::command]
async fn resolve_action_wait(
    state: State<'_, std::sync::Arc<action_ipc::ActionIpc>>,
    wait_id: String,
    ok: bool,
    summary: String,
) -> Result<bool, String> {
    let waits = state.waits();
    Ok(action_ipc::resolve_wait(
        &waits,
        action_ipc::CardResolution { wait_id, ok, summary },
    )
    .await)
}

#[tauri::command]
async fn github_connect_pat(token: String) -> Result<GithubUser, String> {
    let trimmed = token.trim().to_string();
    if trimmed.is_empty() {
        return Err("token is empty".into());
    }
    let (user, _rate_limit) = github::fetch_user(&trimmed).await.map_err(|e| e.to_string())?;
    keychain::set(GITHUB_KEY, &trimmed).map_err(|e| e.to_string())?;
    let _ = cursor_mcp::sync();
    Ok(user)
}

#[tauri::command]
async fn github_status() -> Result<ConnectionStatus, String> {
    match keychain::get(GITHUB_KEY).map_err(|e| e.to_string())? {
        None => Ok(ConnectionStatus::Disconnected),
        Some(t) => match github::fetch_user(&t).await {
            Ok((user, rate_limit)) => Ok(ConnectionStatus::Connected { user, rate_limit }),
            Err(github::GithubError::InvalidToken) => {
                let _ = keychain::delete(GITHUB_KEY);
                Ok(ConnectionStatus::Disconnected)
            }
            Err(e) => Err(e.to_string()),
        },
    }
}

#[tauri::command]
fn github_disconnect() -> Result<(), String> {
    keychain::delete(GITHUB_KEY).map_err(|e| e.to_string())?;
    let _ = cursor_mcp::sync();
    Ok(())
}

/// Idempotent re-sync of `~/.cursor/mcp.json` with whatever creds are
/// currently in Keychain. Wired into every connect/disconnect so Cursor
/// sees the same Jira/GitHub/Sentry/Memory tools as Claude with no
/// manual `cursor-agent mcp add` step. Best-effort — a failure here
/// just means Cursor stays out of sync; doesn't break the connect flow.
#[tauri::command]
fn cursor_mcp_sync() -> Result<Vec<String>, String> {
    cursor_mcp::sync()
}

#[tauri::command]
async fn github_list_inbox() -> Result<Vec<InboxItem>, String> {
    let t = token().await?;
    github::search_involves_me(&t).await.map_err(|e| e.to_string())
}

/// Run a pre-built GitHub search query (`q=` as composed by the frontend).
/// Caller is responsible for combining filters — e.g. `involves:@me is:open`.
#[tauri::command]
async fn github_search_inbox(query: String) -> Result<Vec<InboxItem>, String> {
    let t = token().await?;
    github::search_issues_with_query(&t, &query)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_repos() -> Result<Vec<Repository>, String> {
    let t = token().await?;
    github::list_repos(&t).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_repo_items(
    owner: String,
    repo: String,
    state: String,
) -> Result<Vec<InboxItem>, String> {
    let t = token().await?;
    github::list_repo_issues(&t, &owner, &repo, &state).await.map_err(|e| e.to_string())
}

/// Single PR or issue lookup by `(owner, repo, number)`. Used by the
/// frontend's app-navigation handler when the agent calls
/// `mcp__app__open_github_pr` so we can slot the item into the focus
/// pane without first scrolling through an inbox.
#[tauri::command]
async fn github_get_inbox_item(
    owner: String,
    repo: String,
    number: u64,
) -> Result<InboxItem, String> {
    let t = token().await?;
    github::fetch_inbox_item(&t, &owner, &repo, number)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_workflow_runs(
    owner: String,
    repo: String,
) -> Result<Vec<WorkflowRun>, String> {
    let t = token().await?;
    github::list_workflow_runs(&t, &owner, &repo).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_rerun_workflow(owner: String, repo: String, run_id: u64) -> Result<(), String> {
    let t = token().await?;
    github::rerun_workflow(&t, &owner, &repo, run_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_cancel_workflow(owner: String, repo: String, run_id: u64) -> Result<(), String> {
    let t = token().await?;
    github::cancel_workflow(&t, &owner, &repo, run_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_tree(
    owner: String,
    repo: String,
    reference: String,
) -> Result<Vec<TreeEntry>, String> {
    let t = token().await?;
    github::list_tree(&t, &owner, &repo, &reference).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_get_file_content(
    owner: String,
    repo: String,
    path: String,
    reference: String,
) -> Result<FileBlob, String> {
    let t = token().await?;
    github::get_file_content(&t, &owner, &repo, &path, &reference).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_releases(owner: String, repo: String) -> Result<Vec<Release>, String> {
    let t = token().await?;
    github::list_releases(&t, &owner, &repo).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_repo_commits(
    owner: String,
    repo: String,
    reference: String,
    limit: u32,
) -> Result<Vec<RepoCommit>, String> {
    let t = token().await?;
    github::list_commits(&t, &owner, &repo, &reference, limit).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_repo_branches(owner: String, repo: String) -> Result<Vec<RepoBranch>, String> {
    let t = token().await?;
    github::list_branches_api(&t, &owner, &repo).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_get_readme(
    owner: String,
    repo: String,
) -> Result<Option<RepoReadme>, String> {
    let t = token().await?;
    github::get_readme(&t, &owner, &repo).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_get_pr(owner: String, repo: String, number: u64) -> Result<PrDetail, String> {
    let t = token().await?;
    github::get_pr_detail(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_pr_files(
    owner: String,
    repo: String,
    number: u64,
) -> Result<Vec<ChangedFile>, String> {
    let t = token().await?;
    github::list_pr_files(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_pr_commits(
    owner: String,
    repo: String,
    number: u64,
) -> Result<Vec<CommitEntry>, String> {
    let t = token().await?;
    github::list_pr_commits(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_check_runs(
    owner: String,
    repo: String,
    reference: String,
) -> Result<Vec<CheckRun>, String> {
    let t = token().await?;
    github::list_check_runs(&t, &owner, &repo, &reference)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_pr_reviews(
    owner: String,
    repo: String,
    number: u64,
) -> Result<Vec<Review>, String> {
    let t = token().await?;
    github::list_pr_reviews(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_review_comments(
    owner: String,
    repo: String,
    number: u64,
) -> Result<Vec<ReviewComment>, String> {
    let t = token().await?;
    github::list_pr_review_comments(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_comments(
    owner: String,
    repo: String,
    number: u64,
) -> Result<Vec<Comment>, String> {
    let t = token().await?;
    github::list_issue_comments(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_get_commit(
    owner: String,
    repo: String,
    sha: String,
) -> Result<CommitDetail, String> {
    let t = token().await?;
    github::get_commit(&t, &owner, &repo, &sha).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_add_comment(
    owner: String,
    repo: String,
    number: u64,
    body: String,
) -> Result<Comment, String> {
    let t = token().await?;
    github::add_issue_comment(&t, &owner, &repo, number, &body)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_submit_review(
    owner: String,
    repo: String,
    number: u64,
    event: String,
    body: String,
) -> Result<Review, String> {
    let t = token().await?;
    github::submit_review(&t, &owner, &repo, number, &event, &body)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_set_state(
    owner: String,
    repo: String,
    number: u64,
    state: String,
) -> Result<(), String> {
    let t = token().await?;
    github::set_issue_state(&t, &owner, &repo, number, &state)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_merge_pr(
    owner: String,
    repo: String,
    number: u64,
    method: String,
) -> Result<(), String> {
    let t = token().await?;
    github::merge_pr(&t, &owner, &repo, number, &method).await.map_err(|e| e.to_string())
}

/// PATCH a PR's title and/or body. Either or both can be provided;
/// missing keys leave the corresponding field unchanged on GitHub.
/// Empty strings DO clear the body — pass `null`/None to skip
/// updating that field instead.
#[tauri::command]
async fn github_edit_pr(
    owner: String,
    repo: String,
    number: u64,
    title: Option<String>,
    body: Option<String>,
) -> Result<(), String> {
    let t = token().await?;
    github::edit_pr(&t, &owner, &repo, number, title.as_deref(), body.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_request_reviewers(
    owner: String,
    repo: String,
    number: u64,
    #[allow(non_snake_case)] userLogins: Option<Vec<String>>,
    #[allow(non_snake_case)] teamSlugs: Option<Vec<String>>,
) -> Result<(), String> {
    let t = token().await?;
    github::request_pr_reviewers(
        &t,
        &owner,
        &repo,
        number,
        &userLogins.unwrap_or_default(),
        &teamSlugs.unwrap_or_default(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_remove_reviewers(
    owner: String,
    repo: String,
    number: u64,
    #[allow(non_snake_case)] userLogins: Option<Vec<String>>,
    #[allow(non_snake_case)] teamSlugs: Option<Vec<String>>,
) -> Result<(), String> {
    let t = token().await?;
    github::remove_pr_reviewers(
        &t,
        &owner,
        &repo,
        number,
        &userLogins.unwrap_or_default(),
        &teamSlugs.unwrap_or_default(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_add_labels(
    owner: String,
    repo: String,
    number: u64,
    labels: Vec<String>,
) -> Result<(), String> {
    let t = token().await?;
    github::add_issue_labels(&t, &owner, &repo, number, &labels)
        .await
        .map_err(|e| e.to_string())
}

/// Remove labels from an issue/PR. Loops over the slice to call
/// `remove_issue_label` once per label so a missing-label 404 on one
/// doesn't abort the rest. Returns the labels that successfully
/// removed (caller can diff against the requested set to learn which
/// were already absent).
#[tauri::command]
async fn github_remove_labels(
    owner: String,
    repo: String,
    number: u64,
    labels: Vec<String>,
) -> Result<Vec<String>, String> {
    let t = token().await?;
    let mut removed = Vec::with_capacity(labels.len());
    for l in &labels {
        match github::remove_issue_label(&t, &owner, &repo, number, l).await {
            Ok(()) => removed.push(l.clone()),
            // Surface 404 / not-found as "absent, that's fine"; other
            // errors abort with diagnostic so the caller can act.
            Err(github::GithubError::Api { status: 404, .. }) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(removed)
}

#[tauri::command]
async fn github_add_assignees(
    owner: String,
    repo: String,
    number: u64,
    logins: Vec<String>,
) -> Result<(), String> {
    let t = token().await?;
    github::add_issue_assignees(&t, &owner, &repo, number, &logins)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_remove_assignees(
    owner: String,
    repo: String,
    number: u64,
    logins: Vec<String>,
) -> Result<(), String> {
    let t = token().await?;
    github::remove_issue_assignees(&t, &owner, &repo, number, &logins)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_set_pr_draft(
    owner: String,
    repo: String,
    number: u64,
    draft: bool,
) -> Result<(), String> {
    let t = token().await?;
    github::set_pr_draft(&t, &owner, &repo, number, draft)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_compare(
    owner: String,
    repo: String,
    base: String,
    head: String,
) -> Result<CompareResult, String> {
    let t = token().await?;
    github::compare_branches(&t, &owner, &repo, &base, &head)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_create_pr(
    owner: String,
    repo: String,
    title: String,
    body: String,
    base: String,
    head: String,
    draft: bool,
) -> Result<InboxItem, String> {
    let t = token().await?;
    github::create_pr_item(&t, &owner, &repo, &title, &body, &head, &base, draft)
        .await
        .map_err(|e| e.to_string())
}

// ---------- Jira ----------

#[tauri::command]
async fn jira_connect(
    workspace: String,
    email: String,
    token: String,
) -> Result<JiraUser, String> {
    let creds = JiraCredentials {
        workspace: jira::normalize_workspace(&workspace),
        email: email.trim().to_string(),
        token: token.trim().to_string(),
    };
    if creds.workspace.is_empty() {
        return Err("workspace URL is required".into());
    }
    if creds.email.is_empty() {
        return Err("email is required".into());
    }
    if creds.token.is_empty() {
        return Err("API token is required".into());
    }
    let user = jira::fetch_myself(&creds).await.map_err(|e| e.to_string())?;
    let payload = serde_json::to_string(&creds).map_err(|e| e.to_string())?;
    keychain::set(JIRA_KEY, &payload).map_err(|e| e.to_string())?;
    let _ = cursor_mcp::sync();
    Ok(user)
}

#[tauri::command]
async fn jira_status() -> Result<JiraStatus, String> {
    let Some(stored) = keychain::get(JIRA_KEY).map_err(|e| e.to_string())? else {
        return Ok(JiraStatus::Disconnected);
    };
    let creds: JiraCredentials = match serde_json::from_str(&stored) {
        Ok(c) => c,
        Err(_) => {
            let _ = keychain::delete(JIRA_KEY);
            return Ok(JiraStatus::Disconnected);
        }
    };
    match jira::fetch_myself(&creds).await {
        Ok(user) => Ok(JiraStatus::Connected { user }),
        Err(jira::JiraError::InvalidCredentials) => {
            let _ = keychain::delete(JIRA_KEY);
            Ok(JiraStatus::Disconnected)
        }
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn jira_disconnect() -> Result<(), String> {
    keychain::delete(JIRA_KEY).map_err(|e| e.to_string())?;
    let _ = cursor_mcp::sync();
    Ok(())
}

async fn jira_creds() -> Result<JiraCredentials, String> {
    let stored = keychain::get(JIRA_KEY)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Jira is not connected".to_string())?;
    serde_json::from_str(&stored).map_err(|e| e.to_string())
}

// ---------- Sentry ----------

#[tauri::command]
async fn sentry_connect(
    host: String,
    organization_slug: String,
    token: String,
) -> Result<SentryUser, String> {
    let creds = SentryCredentials {
        host: sentry::normalize_host(&host),
        organization_slug: organization_slug.trim().to_string(),
        token: token.trim().to_string(),
    };
    if creds.organization_slug.is_empty() {
        return Err("organization slug is required (e.g. 'acme-co')".into());
    }
    if creds.token.is_empty() {
        return Err("auth token is required".into());
    }
    let user = sentry::validate(&creds).await?;
    let payload = serde_json::to_string(&creds).map_err(|e| e.to_string())?;
    keychain::set(SENTRY_KEY, &payload).map_err(|e| e.to_string())?;
    let _ = cursor_mcp::sync();
    Ok(user)
}

#[tauri::command]
async fn sentry_status() -> Result<SentryConnectionStatus, String> {
    let Some(stored) = keychain::get(SENTRY_KEY).map_err(|e| e.to_string())? else {
        return Ok(SentryConnectionStatus::Disconnected);
    };
    let creds: SentryCredentials = match serde_json::from_str(&stored) {
        Ok(c) => c,
        Err(_) => {
            let _ = keychain::delete(SENTRY_KEY);
            return Ok(SentryConnectionStatus::Disconnected);
        }
    };
    match sentry::validate(&creds).await {
        Ok(user) => Ok(SentryConnectionStatus::Connected { user }),
        Err(_) => {
            // Token revoked / network blip — leave creds in keychain so the
            // user can retry; surface as disconnected for UX.
            Ok(SentryConnectionStatus::Disconnected)
        }
    }
}

#[tauri::command]
fn sentry_disconnect() -> Result<(), String> {
    keychain::delete(SENTRY_KEY).map_err(|e| e.to_string())?;
    let _ = cursor_mcp::sync();
    Ok(())
}

async fn sentry_creds() -> Result<SentryCredentials, String> {
    let stored = keychain::get(SENTRY_KEY)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Sentry is not connected".to_string())?;
    serde_json::from_str(&stored).map_err(|e| e.to_string())
}

#[tauri::command]
async fn sentry_list_issues(
    query: Option<String>,
    project_slugs: Option<Vec<String>>,
    environment: Option<String>,
    sort: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<SentryIssue>, String> {
    let creds = sentry_creds().await?;
    sentry::list_issues(
        &creds,
        query.as_deref(),
        project_slugs.as_deref().unwrap_or(&[]),
        environment.as_deref(),
        sort.as_deref().unwrap_or("date"),
        limit.unwrap_or(50),
    )
    .await
}

#[tauri::command]
async fn sentry_get_issue(issue_id: String) -> Result<SentryIssue, String> {
    let creds = sentry_creds().await?;
    sentry::get_issue(&creds, &issue_id).await
}

#[tauri::command]
async fn sentry_list_events(issue_id: String, limit: Option<u32>) -> Result<Vec<SentryEvent>, String> {
    let creds = sentry_creds().await?;
    sentry::list_events(&creds, &issue_id, limit.unwrap_or(20)).await
}

#[tauri::command]
async fn sentry_list_projects() -> Result<Vec<SentryProject>, String> {
    let creds = sentry_creds().await?;
    sentry::list_projects(&creds).await
}

#[tauri::command]
async fn sentry_list_environments(project_slug: String) -> Result<Vec<SentryEnvironment>, String> {
    let creds = sentry_creds().await?;
    sentry::list_environments(&creds, &project_slug).await
}

#[tauri::command]
async fn sentry_get_event_detail(
    issue_id: String,
    event_id: Option<String>,
) -> Result<SentryEventDetail, String> {
    let creds = sentry_creds().await?;
    sentry::get_event_detail(&creds, &issue_id, event_id.as_deref().unwrap_or("latest")).await
}

#[tauri::command]
async fn sentry_set_status(issue_id: String, status: String) -> Result<SentryIssue, String> {
    let creds = sentry_creds().await?;
    sentry::set_issue_status(&creds, &issue_id, &status).await
}

#[tauri::command]
async fn jira_list_inbox() -> Result<Vec<JiraItem>, String> {
    let creds = jira_creds().await?;
    jira::list_my_issues(&creds).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_list_inbox_for(
    assignee_account_id: Option<String>,
) -> Result<Vec<JiraItem>, String> {
    let creds = jira_creds().await?;
    jira::list_issues_for(&creds, assignee_account_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Run a pre-built JQL query (composed by the frontend from its filter state).
#[tauri::command]
async fn jira_search(jql: String) -> Result<Vec<JiraItem>, String> {
    let creds = jira_creds().await?;
    jira::search_issues(&creds, &jql).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_list_projects() -> Result<Vec<JiraProject>, String> {
    let creds = jira_creds().await?;
    jira::list_projects(&creds).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_list_boards(project_key: Option<String>) -> Result<Vec<JiraBoard>, String> {
    let creds = jira_creds().await?;
    jira::list_boards(&creds, project_key.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_list_sprints(board_id: u64) -> Result<Vec<JiraSprint>, String> {
    let creds = jira_creds().await?;
    jira::list_sprints(&creds, board_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_list_statuses(
    project_key: Option<String>,
) -> Result<Vec<JiraWorkflowStatus>, String> {
    let creds = jira_creds().await?;
    jira::list_statuses(&creds, project_key.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_list_issue_types(project_key: String) -> Result<Vec<JiraIssueType>, String> {
    let creds = jira_creds().await?;
    jira::list_issue_types(&creds, &project_key).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_create_issue(
    project_key: String,
    issue_type: String,
    summary: String,
    description: String,
    assignee_account_id: Option<String>,
    sprint_id: Option<u64>,
) -> Result<JiraItem, String> {
    let creds = jira_creds().await?;
    jira::create_issue(
        &creds,
        &project_key,
        &issue_type,
        &summary,
        &description,
        assignee_account_id.as_deref(),
        sprint_id,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_search_users(query: String) -> Result<Vec<JiraUserSummary>, String> {
    let creds = jira_creds().await?;
    jira::search_users(&creds, &query).await.map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
async fn jira_list_assignable_users(projectKey: String) -> Result<Vec<JiraUserSummary>, String> {
    let creds = jira_creds().await?;
    jira::list_assignable_users(&creds, &projectKey).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_set_assignee(key: String, account_id: Option<String>) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::set_assignee(&creds, &key, account_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_set_priority(key: String, priority: String) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::set_priority(&creds, &key, &priority).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_set_labels(key: String, labels: Vec<String>) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::set_labels(&creds, &key, labels).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_get_issue_detail(key: String) -> Result<JiraDetail, String> {
    let creds = jira_creds().await?;
    jira::get_issue_detail(&creds, &key).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_update_issue(
    key: String,
    summary: Option<String>,
    description: Option<String>,
) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::update_issue(&creds, &key, summary.as_deref(), description.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_transition_issue(key: String, transition_id: String) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::transition_issue(&creds, &key, &transition_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_add_comment(key: String, body: String) -> Result<JiraComment, String> {
    let creds = jira_creds().await?;
    jira::add_comment(&creds, &key, &body).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_list_worklogs(key: String) -> Result<Vec<JiraWorklog>, String> {
    let creds = jira_creds().await?;
    jira::list_worklogs(&creds, &key).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_add_worklog(
    key: String,
    #[allow(non_snake_case)] timeSpentSeconds: i64,
    started: Option<String>,
    comment: Option<String>,
) -> Result<JiraWorklog, String> {
    let creds = jira_creds().await?;
    jira::add_worklog(
        &creds,
        &key,
        timeSpentSeconds,
        started.as_deref(),
        comment.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_delete_worklog(
    key: String,
    #[allow(non_snake_case)] worklogId: String,
) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::delete_worklog(&creds, &key, &worklogId).await.map_err(|e| e.to_string())
}

// ---------- Claude Code ----------

#[tauri::command]
fn claude_status() -> ClaudeStatus {
    claude::detect()
}

#[tauri::command]
async fn claude_ask(
    app: tauri::AppHandle,
    runners: State<'_, Runners>,
    warm_pool: State<'_, WarmPool>,
    ipc: State<'_, std::sync::Arc<action_ipc::ActionIpc>>,
    session_id: String,
    prompt: String,
    cwd: Option<String>,
    claude_uuid: String,
    resume: bool,
    rules: Option<String>,
    // camelCase on the wire (Svelte invokes pass JS-style keys); snake_case
    // is what Tauri's codegen expects, so spell the param out explicitly to
    // keep the frontend contract stable.
    #[allow(non_snake_case)] agentKind: Option<AgentKind>,
    #[allow(non_snake_case)] cursorModel: Option<String>,
    // Forwarded as `--model <id>` to claude CLI. None means no flag → CLI
    // picks its default (Opus on Max). Frontend defaults new sessions to
    // `claude-sonnet-4-6` so the typical case doesn't burn the 5h quota.
    #[allow(non_snake_case)] claudeModel: Option<String>,
    // Per-turn dynamic context describing the agent's UI surroundings —
    // the active solo, sibling app instances + their names + cwds, and
    // which instance the calling session is bound to. Built fresh on the
    // frontend before every turn (instance map changes, sessions change
    // cwd) so the agent always sees current state. Prepended to the
    // system prompt for Claude / to the prompt itself for Cursor.
    #[allow(non_snake_case)] appContext: Option<String>,
    // Absolute paths of image files attached to this turn. For Claude these
    // get base64-embedded as `image` content blocks via stream-json input
    // (the model sees the bytes directly via vision). Empty for Cursor —
    // its CLI has no equivalent input format, so the frontend falls back
    // to the path-mention flow there.
    #[allow(non_snake_case)] imagePaths: Option<Vec<String>>,
) -> Result<AgentAskResult, String> {
    let cwd_path = cwd.as_deref().map(std::path::Path::new);
    let kind = agentKind.unwrap_or_default();
    let images = imagePaths.unwrap_or_default();
    let ipc_socket = ipc.inner().socket_path().to_path_buf();
    let result = agent::ask(
        kind,
        app,
        runners.inner().clone(),
        warm_pool.inner().clone(),
        &session_id,
        &prompt,
        cwd_path,
        &claude_uuid,
        resume,
        rules.as_deref(),
        cursorModel.as_deref(),
        claudeModel.as_deref(),
        appContext.as_deref(),
        Some(ipc_socket.as_path()),
        &images,
    )
    .await;
    // Tag resume-orphan with a stable prefix on the wire so the frontend
    // can recognise it without parsing free-form CLI stderr. Anything
    // else passes through as-is (the frontend already has decent error
    // toasts for the common cases).
    match result {
        Ok(r) => Ok(r),
        Err(e) if e.is_resume_orphan() => Err(format!("RESUME_ORPHAN: {}", e)),
        Err(e) => Err(e.to_string()),
    }
}

/// Pre-spawn a `claude` CLI for `session_id` so the cold-start cost
/// (binary load + `--resume` history hydration) overlaps with the user
/// typing. Idempotent for matching args. Cheap to call frequently —
/// the implementation no-ops when the existing warm entry is still
/// good. Frontend triggers this on textarea focus / first keystroke.
#[tauri::command]
async fn claude_prewarm(
    warm_pool: State<'_, WarmPool>,
    ipc: State<'_, std::sync::Arc<action_ipc::ActionIpc>>,
    session_id: String,
    cwd: Option<String>,
    claude_uuid: String,
    resume: bool,
    rules: Option<String>,
    #[allow(non_snake_case)] agentKind: Option<AgentKind>,
    #[allow(non_snake_case)] claudeModel: Option<String>,
    #[allow(non_snake_case)] appContext: Option<String>,
) -> Result<(), String> {
    // Cursor CLI takes its prompt as a positional arg, so we have to
    // know the prompt at spawn time — pre-warming cursor-agent would
    // mean spawning with a placeholder prompt the user never sent.
    // Skip the call for cursor sessions; cold-path `ask` still works.
    if agentKind.unwrap_or_default() == AgentKind::Cursor {
        return Ok(());
    }
    let cwd_path = cwd.as_deref().map(std::path::Path::new);
    let ipc_socket = ipc.inner().socket_path().to_path_buf();
    claude::prewarm(
        warm_pool.inner().clone(),
        &session_id,
        cwd_path,
        &claude_uuid,
        resume,
        rules.as_deref(),
        claudeModel.as_deref(),
        appContext.as_deref(),
        Some(ipc_socket.as_path()),
    )
    .await
    .map_err(|e| e.to_string())
}

/// Drop any warm CLI parked for `session_id`. Frontend calls this on
/// session/tab switch, cwd change, blur-after-empty-input, and chat
/// deletion — anywhere the parked CLI is no longer the right fit for
/// what the user might do next. Safe to call when no entry exists.
#[tauri::command]
async fn claude_drop_prewarm(
    warm_pool: State<'_, WarmPool>,
    session_id: String,
) -> Result<(), String> {
    claude::drop_prewarm(warm_pool.inner().clone(), &session_id).await;
    Ok(())
}

#[tauri::command]
fn agent_status() -> AgentStatus {
    agent::detect_all()
}

/// Subscription / plan-usage panel — same numbers the Claude Code CLI
/// `/usage` command shows (5-hour limit, weekly all-models, Sonnet-
/// only, Opus-only, Claude Design). Reads the OAuth token from the
/// system keychain and calls Anthropic's undocumented oauth/usage
/// endpoint. Frontend should cache aggressively (60s+) — endpoint
/// 429s under tight polling.
#[tauri::command]
async fn claude_plan_usage() -> Result<claude_quota::PlanUsage, String> {
    claude_quota::fetch_plan_usage()
        .await
        .map_err(|e| e.to_string())
}

/// Fork-compact dispatcher: runs the kind-appropriate two-shot summary
/// → seed flow (claude.rs or cursor.rs) and returns `{ new_uuid,
/// summary }`. Frontend swaps the session's stored uuid to `new_uuid`
/// (which equals `proposed_new_uuid` for claude; cursor mints its own
/// and round-trips it). Replaces the older `claude_compact_session`
/// command so cursor sessions can compact too.
#[tauri::command]
async fn agent_compact_session(
    #[allow(non_snake_case)] agentKind: AgentKind,
    #[allow(non_snake_case)] oldUuid: String,
    #[allow(non_snake_case)] proposedNewUuid: String,
    cwd: Option<String>,
    model: Option<String>,
) -> Result<claude::CompactResult, String> {
    let cwd_path = cwd.as_deref().map(std::path::Path::new);
    agent::compact_session(
        agentKind,
        &oldUuid,
        &proposedNewUuid,
        cwd_path,
        model.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn claude_stop(runners: State<'_, Runners>, session_id: String) -> Result<bool, String> {
    // Try both adapters — whichever one owns the PID for this session does
    // the kill. Keeps the command name backwards-compatible even though it
    // no longer targets only Claude.
    Ok(agent::stop(&runners, &session_id))
}

#[tauri::command]
async fn agent_generate_commit_message(
    repo: String,
    #[allow(non_snake_case)] agentKind: AgentKind,
) -> Result<String, String> {
    let path = std::path::PathBuf::from(&repo);
    agent::generate_commit_message(agentKind, &path)
        .await
        .map_err(|e| e.to_string())
}

// ---------- FS ----------

// ---- Library (skills / plugins store) --------------------------------

#[tauri::command]
fn library_list_installed() -> Result<library::InstalledList, String> {
    Ok(library::list_installed())
}

#[tauri::command]
fn library_install_skill_git(
    url: String,
    slug: String,
) -> Result<library::InstalledSkill, String> {
    library::install_skill_git(&url, &slug)
}

#[tauri::command]
fn library_install_skill_inline(
    slug: String,
    content: String,
) -> Result<library::InstalledSkill, String> {
    library::install_skill_inline(&slug, &content)
}

#[tauri::command]
fn library_install_anthropic_skill(name: String) -> Result<library::InstalledSkill, String> {
    library::install_anthropic_skill(&name)
}

#[tauri::command]
fn library_install_skill_from_repo(
    repo: String,
    slug: String,
    root: Option<String>,
) -> Result<library::InstalledSkill, String> {
    let r = root.as_deref().unwrap_or("skills");
    library::install_skill_from_repo(&repo, &slug, r)
}

#[tauri::command]
fn library_plugin_install_anthropic(name: String) -> Result<String, String> {
    library::plugin_install_anthropic(&name)
}

#[tauri::command]
fn library_uninstall_skill(slug: String) -> Result<(), String> {
    library::uninstall_skill(&slug)
}

#[tauri::command]
fn library_plugin_marketplace_add(url: String) -> Result<String, String> {
    library::plugin_marketplace_add(&url)
}

#[tauri::command]
fn library_plugin_install(reference: String) -> Result<String, String> {
    library::plugin_install(&reference)
}

#[tauri::command]
fn library_plugin_uninstall(name: String) -> Result<String, String> {
    library::plugin_uninstall(&name)
}

#[tauri::command]
fn fs_read_file(path: String) -> Result<String, String> {
    fs::read_file(&path)
}

#[tauri::command]
fn fs_write_file(path: String, contents: String) -> Result<(), String> {
    fs::write_file(&path, &contents)
}

/// Generic single-file delete. Idempotent on missing paths. Used by
/// the canvas-on-disk garbage collection (`canvasState`) and any
/// other caller that needs to drop a file without surfacing a
/// "doesn't exist" error.
#[tauri::command]
fn fs_remove_file(path: String) -> Result<(), String> {
    fs::remove_file_if_exists(&path)
}

/// Recursively delete a directory and all its contents. Used by the
/// FileTree right-click "Delete folder" path. The frontend already
/// gated the action behind a confirm() dialog; the safety net here
/// is the depth check inside `fs::remove_dir_recursive` which keeps
/// a misclick from nuking a system folder.
#[tauri::command]
fn fs_remove_dir(path: String) -> Result<(), String> {
    fs::remove_dir_recursive(&path)
}

/// Rename / move a path. Refuses to overwrite an existing
/// destination so a caller "rename to existing-name" doesn't
/// silently nuke the other file. Used by the FileTree right-click
/// menu (M4 §2.1.2).
#[tauri::command]
fn fs_rename(from: String, to: String) -> Result<(), String> {
    use std::path::Path;
    let from_p = Path::new(&from);
    let to_p = Path::new(&to);
    if !from_p.exists() {
        return Err(format!("source {from} does not exist"));
    }
    if to_p.exists() {
        return Err(format!("destination {to} already exists"));
    }
    std::fs::rename(from_p, to_p).map_err(|e| format!("rename {from} -> {to}: {e}"))
}

/// Open the system file manager scrolled to and highlighting the
/// given path. macOS-only via `open -R` ("reveal"). On other
/// platforms we'd need a different shell-out; not in 1.0 scope.
#[tauri::command]
fn fs_reveal_in_finder(path: String) -> Result<(), String> {
    let status = std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .status()
        .map_err(|e| format!("spawn open: {e}"))?;
    if !status.success() {
        return Err(format!("open -R exited with status {status}"));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct SidecarHealth {
    pub name: String,
    pub running: bool,
    pub pid_count: usize,
}

/// Snapshot of which Woom MCP sidecars are alive. Sidecars are
/// spawned by Claude / Cursor on first MCP handshake (not by us),
/// so a `running: false` row means no agent has yet asked that
/// sidecar to start in this session — not that it crashed. Drives
/// the Settings → "MCP servers" diagnostic card (M4 §2.9.8).
#[tauri::command]
fn mcp_sidecar_health() -> Vec<SidecarHealth> {
    let names = [
        "woom-app",
        "woom-github",
        "woom-jira",
        "woom-sentry",
        "woom-memory",
    ];
    names
        .iter()
        .map(|name| {
            /* `pgrep -fc` returns the count of matches on stdout (or 0
             * exit + empty when nothing matched, depending on the
             * pgrep flavor). We just count newlines from `pgrep -f`
             * since `-c` semantics differ between BSD/macOS pgrep
             * and Linux pgrep. */
            let count = std::process::Command::new("pgrep")
                .arg("-f")
                .arg(name)
                .output()
                .ok()
                .map(|out| {
                    String::from_utf8_lossy(&out.stdout)
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .count()
                })
                .unwrap_or(0);
            SidecarHealth {
                name: (*name).to_string(),
                running: count > 0,
                pid_count: count,
            }
        })
        .collect()
}

/// Revert one Edit / MultiEdit chunk: replace `new_text` (what the agent
/// wrote) back with `old_text` (what was there before). Behaviour:
///
/// - File is missing → `Err("file not found")`. The agent might have
///   moved/deleted it between the Edit and the user's Revert click; we
///   refuse to recreate a phantom rather than silently land an empty
///   file.
/// - `new_text` not present → `Err(...)`. Either a later edit overlaid
///   ours, or the user already reverted manually. Surfacing the error
///   lets the diff card flip to `error` state with a usable note,
///   instead of a no-op write that lies about success.
/// - `new_text` appears multiple times → `Err(...)`. Edit only emits
///   when its `old_string` was unique, so a multi-match means the file
///   has drifted into something we can't safely target. Refusing here
///   prevents stomping on legit later edits to a *different* hit.
/// - Single match → in-place replace, write back.
///
/// Atomic from the user's perspective (single `write_file` after the
/// match check), but not crash-safe — if the host crashes between
/// `read_file` and `write_file` the file is unchanged. We accept that
/// because Tauri's local file API doesn't surface fsync semantics
/// across platforms, and an Edit revert losing power-cycle data would
/// be a far weirder failure than the original Edit losing it.
#[tauri::command]
fn revert_edit(
    #[allow(non_snake_case)] filePath: String,
    #[allow(non_snake_case)] oldText: String,
    #[allow(non_snake_case)] newText: String,
) -> Result<(), String> {
    let current = fs::read_file(&filePath)?;
    if newText.is_empty() {
        // Defensive: an Edit with empty new_string would mean "delete
        // this slice", and we'd happily insert old_text everywhere —
        // including positions the agent never touched. Refuse and let
        // the user resolve manually.
        return Err("cannot revert an edit whose new_string was empty".into());
    }
    let count = current.matches(newText.as_str()).count();
    if count == 0 {
        return Err(
            "the new_text isn't in the file anymore — a later edit must have overlaid it. Refusing to revert.".into()
        );
    }
    if count > 1 {
        return Err(format!(
            "the new_text matches {} places in the file — Edit normally requires unique matches; refusing to revert ambiguously.",
            count
        ));
    }
    let next = current.replacen(newText.as_str(), oldText.as_str(), 1);
    fs::write_file(&filePath, &next)
}

/// Revert one `Write` (full-file overwrite). Differs from `revert_edit`
/// in three ways:
///   1. The "before" text is the full file, not a unique substring, so
///      we don't search-and-replace — we just rewrite (or delete).
///   2. We still verify the current file equals `newText` before
///      touching it. If something edited the file *after* the agent's
///      Write, blindly restoring `oldText` would clobber that work.
///      Surfacing the error lets the user investigate instead of
///      silently losing changes.
///   3. `isCreate=true` is the "agent created this from nothing" case
///      — there's no `oldText` to restore, so the inverse is to
///      delete the file. We bail if the file is missing (already
///      reverted by hand) and if the contents drifted.
///
/// Trade-off: requiring the on-disk content to match `newText` exactly
/// rules out cases where the user added a single character before
/// hitting Revert. Worth it — the alternative (best-effort rewrite)
/// lets a stale Revert click silently overwrite live edits, which is
/// strictly worse than asking the user to revert manually.
#[tauri::command]
fn revert_write(
    #[allow(non_snake_case)] filePath: String,
    #[allow(non_snake_case)] oldText: String,
    #[allow(non_snake_case)] newText: String,
    #[allow(non_snake_case)] isCreate: bool,
) -> Result<(), String> {
    use std::path::Path;
    let path = Path::new(&filePath);
    if !path.exists() {
        // Either the user manually deleted/moved the file, or another
        // tool already reverted. Either way nothing for us to do, and
        // recreating an empty placeholder would be worse than the
        // current state.
        return Err("file not found — nothing to revert".into());
    }
    let current = fs::read_file(&filePath)?;
    if current != newText {
        return Err(
            "the file's contents don't match what the agent wrote — something modified it after the Write. Refusing to revert."
                .into(),
        );
    }
    if isCreate {
        // Safety guardrail before destruction: the FE marks `isCreate=true`
        // optimistically when its backfill (`git show HEAD:<file>`) couldn't
        // recover the pre-state. Two failure modes silently slipped through
        // before this check:
        //   • `git_repo_root(filePath)` was called with the *file* path, not
        //     a directory — git rejects that, the backfill threw, and the
        //     card stayed `isCreate=true`. Revert then deleted committed
        //     files (lost README.md case).
        //   • `git_show` returned empty for any non-HEAD reason (encoding,
        //     LFS, partial clone, …) and we couldn't tell "new file" apart
        //     from "tracked file we failed to read".
        // Resolution: if git knows about the path, refuse the delete and
        // surface a clear error. The user can then either revert manually
        // (knowing the agent did clobber a tracked file) or accept the
        // current contents.
        let parent = path
            .parent()
            .and_then(|p| p.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(".");
        if let Ok(repo_root) = git::repo_root(parent) {
            if !repo_root.is_empty() {
                let rel = path
                    .strip_prefix(&repo_root)
                    .ok()
                    .and_then(|p| p.to_str())
                    .unwrap_or("");
                if !rel.is_empty() && git::is_tracked(&repo_root, rel) {
                    return Err(format!(
                        "refusing to delete '{}': it's tracked in git, so the agent didn't actually create it — our pre-Write baseline lookup failed and we'd be wiping committed content. Restore manually with `git restore -- {}`.",
                        rel, rel
                    ));
                }
            }
        }
        // Inverse of "Write created this file" is "remove the file".
        // Skip directory cleanup — the agent might have created
        // `a/b/c.txt` deep in a fresh tree, but we don't know which
        // ancestors existed before, and removing non-empty dirs is
        // unsafe regardless.
        std::fs::remove_file(path).map_err(|e| format!("failed to delete file: {}", e))
    } else {
        fs::write_file(&filePath, &oldText)
    }
}

/// Restore a file that the agent deleted, using the `prev_content` we
/// captured at deletion time (cursor-agent's `result.success.prevContent`,
/// or a `git show HEAD:<file>` fallback for tracked files). Inverse of
/// `Delete`; the EditDiffCard's "Restore" button calls this.
///
/// Refuses if a file already exists at the path. Two reasons:
///   1. The user might have manually re-created the file (or another
///      tool did). Silently overwriting destroys their work.
///   2. The agent might have deleted-then-re-created in a single turn
///      (a refactor that moves contents elsewhere). The first event's
///      Restore would clobber the second event's work.
/// Refusing forces the user to investigate, which is correct given
/// we have no way to verify the absent state still holds.
///
/// `fs::write_file` creates parent dirs if missing — so restoring a
/// nested path under a now-empty directory tree works (the agent might
/// have deleted `a/b/c.txt` and `a/b/`'s siblings; we re-mkdir `a/b/`).
#[tauri::command]
fn restore_deleted_file(
    #[allow(non_snake_case)] filePath: String,
    #[allow(non_snake_case)] prevContent: String,
) -> Result<(), String> {
    use std::path::Path;
    let path = Path::new(&filePath);
    if path.exists() {
        return Err(
            "a file already exists at this path — refusing to overwrite. Delete it manually first if you want to restore.".into()
        );
    }
    fs::write_file(&filePath, &prevContent)
}

/// Re-delete a previously-restored file. The user clicked "Restore" on
/// a Delete card (flipping it to `reverted`), then changed their mind
/// and clicked "Re-delete". Inverse of `restore_deleted_file`.
///
/// Verifies the on-disk contents still equal `prev_content` before
/// removing. If something modified the file after restore (the user
/// edited it; a later turn touched it), refuses to delete — the safe
/// default is to keep work the user might care about. Surfacing the
/// error lets the diff card flip to `error` state with a usable note,
/// so the user can choose to manually delete or to abandon the
/// re-delete.
#[tauri::command]
fn redelete_file(
    #[allow(non_snake_case)] filePath: String,
    #[allow(non_snake_case)] prevContent: String,
) -> Result<(), String> {
    use std::path::Path;
    let path = Path::new(&filePath);
    if !path.exists() {
        return Err("file is already gone — nothing to delete".into());
    }
    let current = fs::read_file(&filePath)?;
    if current != prevContent {
        return Err(
            "the file's contents have drifted since restore — refusing to delete blindly. Edit, save, or remove the file manually if you want to discard changes."
                .into(),
        );
    }
    std::fs::remove_file(path).map_err(|e| format!("failed to delete file: {}", e))
}

#[tauri::command]
fn fs_write_bytes(path: String, base64: String) -> Result<(), String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64.as_bytes())
        .map_err(|e| format!("invalid base64: {}", e))?;
    fs::write_bytes(&path, &bytes)
}

/// Resolve the OS-level app data dir (`~/Library/Application Support/<id>` on
/// macOS) for the frontend. Used as a stable cwd-independent home for chat
/// image attachments saved from clipboard or Cmd+Shift+5 drag, where we have
/// only the bytes (no source path).
#[tauri::command]
fn app_data_dir(app: tauri::AppHandle) -> Result<String, String> {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

/// Native webview zoom — same code path as Cmd+/- in Cursor / VSCode /
/// Chrome. We use this instead of CSS `zoom` on `<html>` because the
/// CSS approach silently breaks `position: fixed`, viewport units
/// (`100vw` / `100vh`), and scroll-container dimensions in WebKit
/// (Tauri's macOS engine) — at 0.9× zoom modals drift, sidebars
/// undersize, and `getBoundingClientRect` returns post-zoom values
/// while `clientX`/`clientY` are still in CSS pixels. Native
/// webview zoom dilates the entire pixel grid uniformly so all
/// layout math stays consistent.
///
/// `factor` is the multiplier (e.g. `0.8`, `1.0`, `1.25`). We clamp
/// to [0.5, 3.0] to stop frontend bugs from rendering an unusable
/// window. Values outside this range produce truly degenerate UI
/// (text below ~7px is unreadable, above 3× a single column eats
/// the whole window).
#[tauri::command]
fn set_window_zoom(window: tauri::WebviewWindow, factor: f64) -> Result<(), String> {
    let clamped = factor.clamp(0.5, 3.0);
    window.set_zoom(clamped).map_err(|e| e.to_string())
}

#[tauri::command]
fn fs_list_dir(path: String) -> Result<Vec<DirEntry>, String> {
    fs::list_dir(&path)
}

/// Bounded recursive file search — used by the composer's @-mention
/// picker. `query` is a case-insensitive substring on the leaf
/// filename; pass `None` to list everything up to `max_files`.
/// Skips `.git`, `node_modules`, build outputs, and the usual VCS
/// noise so a 5,000-file monorepo stays sub-50ms.
#[tauri::command]
fn fs_walk_files(
    root: String,
    query: Option<String>,
    max_files: Option<u32>,
    max_depth: Option<u32>,
) -> Result<Vec<DirEntry>, String> {
    let mf = max_files.map(|x| x as usize).unwrap_or(2000).clamp(1, 10_000);
    let md = max_depth.map(|x| x as usize).unwrap_or(8).clamp(1, 16);
    fs::walk_files(&root, query.as_deref(), mf, md)
}

/// Project-wide content search — the Editor's ⌘⇧F overlay. Plain
/// case-insensitive substring; binary / oversized files skipped.
/// See `fs::search_text` for the heuristics.
#[tauri::command]
fn fs_search_text(
    root: String,
    query: String,
    max_results: Option<u32>,
) -> Result<fs::SearchTextResult, String> {
    let cap = max_results.map(|x| x as usize).unwrap_or(500).clamp(1, 5_000);
    fs::search_text(&root, &query, cap)
}

#[tauri::command]
fn fs_path_exists(path: String) -> bool {
    fs::path_exists(&path)
}

#[tauri::command]
async fn fs_bash_run(cwd: String, command: String) -> Result<BashResult, String> {
    fs::bash_run(&cwd, &command).await
}

// ---------- Git ----------

#[tauri::command]
fn git_status(repo: String) -> Result<GitStatus, String> {
    git::status(&repo)
}

#[tauri::command]
fn git_check_ignore(repo: String, paths: Vec<String>) -> Vec<String> {
    git::check_ignore(&repo, &paths)
}

#[tauri::command]
fn git_ls_files(repo: String) -> Vec<String> {
    git::ls_files(&repo)
}

#[tauri::command]
fn git_branches(repo: String) -> Result<Vec<Branch>, String> {
    git::branches(&repo)
}

#[tauri::command]
fn git_current_branch(repo: String) -> Result<String, String> {
    git::current_branch(&repo)
}

#[tauri::command]
fn git_checkout(repo: String, branch: String) -> Result<(), String> {
    git::checkout(&repo, &branch)
}

#[tauri::command]
fn git_create_branch(
    repo: String,
    name: String,
    checkout: bool,
    start_point: Option<String>,
) -> Result<(), String> {
    git::create_branch(&repo, &name, checkout, start_point.as_deref())
}

#[tauri::command]
fn git_fetch(repo: String) -> Result<String, String> {
    git::fetch(&repo)
}

#[tauri::command]
fn git_stage(repo: String, paths: Vec<String>) -> Result<(), String> {
    git::stage(&repo, &paths)
}

#[tauri::command]
fn git_unstage(repo: String, paths: Vec<String>) -> Result<(), String> {
    git::unstage(&repo, &paths)
}

#[tauri::command]
fn git_discard(repo: String, paths: Vec<String>) -> Result<(), String> {
    git::discard(&repo, &paths)
}

// Network-touching git commands run through `spawn_blocking` so a slow
// remote (push to GitHub, pull through a VPN, etc.) doesn't park a
// Tauri worker thread for the whole round-trip. Previously these were
// sync `fn` commands — under the hood Tauri scheduled them on its
// blocking pool, but every concurrent commit/push card stole one of
// that pool's threads, and a stack of approve-cards could starve the
// IPC queue. Symptom: action card stuck on "executing" and the UI
// freezing while waiting on `invoke()`. Wrapping the existing sync
// impl in `spawn_blocking` keeps the behaviour identical but
// guarantees the Tokio runtime isn't pinned.
#[tauri::command]
async fn git_commit(repo: String, message: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || git::commit(&repo, &message))
        .await
        .map_err(|e| format!("git_commit join: {}", e))?
}

#[tauri::command]
async fn git_push(repo: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || git::push(&repo))
        .await
        .map_err(|e| format!("git_push join: {}", e))?
}

#[tauri::command]
async fn git_pull(repo: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || git::pull(&repo))
        .await
        .map_err(|e| format!("git_pull join: {}", e))?
}

#[tauri::command]
fn git_log(repo: String, limit: u32) -> Result<Vec<GitCommitEntry>, String> {
    git::log(&repo, limit)
}

#[tauri::command]
fn git_repo_root(path: String) -> Result<String, String> {
    git::repo_root(&path)
}

/// Resolve the pre-write baseline for `file_path` in a single round-trip.
///
/// Replaces the FE's previous "git_repo_root + git_show" pair, which had
/// two latent bugs:
///   1. `git_repo_root` was called with a *file* path, not a directory.
///      `git -C <file>` rejects with ENOTDIR — the call threw, the FE's
///      catch fell through to "isCreate=true", and Revert silently
///      deleted committed files. Here we always resolve the repo from
///      the file's parent directory.
///   2. The FE couldn't tell "tracked but not at HEAD" apart from
///      "untracked / new file" — both produced an empty `oldText`, both
///      stayed `isCreate=true`. We expose `tracked` explicitly so the FE
///      can mark the card as a modify (not a create) even when HEAD has
///      no version of the path (e.g. file was just staged, or in a fresh
///      repo).
///
/// Returns:
///   • `repo_root`: empty when `file_path` isn't inside any git worktree.
///   • `old_text`:  HEAD-version of the file ("" if absent at HEAD or
///                  outside any repo).
///   • `tracked`:   true iff the file is in the index right now. Used as
///                  the authoritative "did this file pre-exist before
///                  the agent's Write" signal.
#[derive(serde::Serialize)]
struct PreWriteBaseline {
    repo_root: String,
    old_text: String,
    tracked: bool,
}

#[tauri::command]
fn pre_write_baseline(#[allow(non_snake_case)] filePath: String) -> PreWriteBaseline {
    use std::path::Path;
    let path = Path::new(&filePath);
    // Resolve repo from the file's *parent directory*. `git -C <dir>` is
    // the only shape that works; `git -C <file>` errors. If parent is
    // missing (e.g. weird absolute root), fall back to "." which lets
    // git use the process cwd.
    let parent = path
        .parent()
        .and_then(|p| p.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(".");
    let repo_root = git::repo_root(parent).unwrap_or_default();
    if repo_root.is_empty() {
        return PreWriteBaseline {
            repo_root: String::new(),
            old_text: String::new(),
            tracked: false,
        };
    }
    let rel = path
        .strip_prefix(&repo_root)
        .ok()
        .and_then(|p| p.to_str())
        .map(String::from)
        .unwrap_or_else(|| filePath.clone());
    let old_text = git::show(&repo_root, "HEAD", &rel).unwrap_or_default();
    let tracked = git::is_tracked(&repo_root, &rel);
    PreWriteBaseline {
        repo_root,
        old_text,
        tracked,
    }
}

#[tauri::command]
fn git_repo_info(path: String) -> RepoInfo {
    git::repo_info(&path)
}

#[tauri::command]
fn git_diff(repo: String, path: String, staged: bool) -> Result<String, String> {
    git::diff(&repo, &path, staged)
}

#[tauri::command]
fn git_show(repo: String, revision: String, path: String) -> Result<String, String> {
    git::show(&repo, &revision, &path)
}

#[tauri::command]
async fn git_commit_and_push(repo: String, message: String) -> Result<String, String> {
    // Same threading rationale as `git_commit` / `git_push`: keep the
    // commit+push pair off the Tokio runtime so a slow `git push` to
    // origin doesn't starve sibling action cards waiting for their own
    // `invoke()` to resolve.
    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let sha = git::commit(&repo, &message)?;
        let push_out = git::push(&repo)?;
        Ok(format!("{}\n{}", sha, push_out.trim()))
    })
    .await
    .map_err(|e| format!("git_commit_and_push join: {}", e))?
}

#[tauri::command]
async fn git_create_pr(
    repo: String,
    title: String,
    body: String,
    draft: bool,
    base: Option<String>,
) -> Result<String, String> {
    let token = token().await?;
    git::create_pr(&repo, &title, &body, draft, base.as_deref(), &token).await
}

#[tauri::command]
fn git_gh_cli_available() -> bool {
    // Kept for any legacy callers; modern PR creation no longer needs the
    // `gh` CLI — it hits the GitHub REST API with the Keychain token.
    git::gh_cli_available()
}

/// True iff a GitHub token is stored in Keychain — i.e. PR creation through
/// `git_create_pr` will work. Cheaper than `github_status` (no API call).
#[tauri::command]
fn pr_create_available() -> bool {
    keychain::get(GITHUB_KEY)
        .ok()
        .flatten()
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false)
}

#[tauri::command]
fn fs_watch_start(
    state: State<'_, WatcherState>,
    app: tauri::AppHandle,
    path: String,
) -> Result<(), String> {
    watch::start(state.inner(), app, &path)
}

#[tauri::command]
fn fs_watch_stop(state: State<'_, WatcherState>) -> Result<(), String> {
    watch::stop(state.inner());
    Ok(())
}

// ---------- Worktree (per-session isolated git worktrees) ----------

#[tauri::command]
fn worktree_create(
    repo: String,
    session_id: String,
    base_ref: Option<String>,
) -> Result<Worktree, String> {
    worktree::create(&repo, &session_id, base_ref.as_deref())
}

#[tauri::command]
fn worktree_remove(repo: String, session_id: String) -> Result<(), String> {
    worktree::remove(&repo, &session_id)
}

#[tauri::command]
fn worktree_list(repo: String) -> Result<Vec<Worktree>, String> {
    worktree::list(&repo)
}

#[derive(Debug, Serialize, Clone)]
struct WorktreeDiff {
    files: Vec<WorktreeChangedFile>,
    raw: String,
}

#[tauri::command]
fn worktree_diff(
    repo: String,
    session_id: String,
    base_ref: Option<String>,
) -> Result<WorktreeDiff, String> {
    let (files, raw) = worktree::diff(&repo, &session_id, base_ref.as_deref())?;
    Ok(WorktreeDiff { files, raw })
}

#[tauri::command]
fn worktree_apply(repo: String, session_id: String) -> Result<String, String> {
    worktree::apply(&repo, &session_id)
}

#[tauri::command]
fn worktree_disk_usage() -> u64 {
    worktree::disk_usage_bytes()
}

#[tauri::command]
fn worktree_storage_dir() -> Option<String> {
    worktree::storage_root().map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn worktree_cleanup_orphans(
    active_session_ids: Vec<String>,
    max_age_secs: u64,
) -> worktree::CleanupSummary {
    worktree::cleanup_orphans(&active_session_ids, max_age_secs)
}

// ---------- Biometry (Touch ID) ----------

#[tauri::command]
async fn biometric_unlock(reason: Option<String>) -> Result<(), String> {
    let reason = reason.unwrap_or_else(|| "Unlock Woom".to_string());
    biometry::authenticate(&reason).await
}
