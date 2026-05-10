//! Claude Code CLI detection.
//!
//! Woom does NOT own Claude Code authentication — the `claude` CLI manages
//! its own auth (subscription via `claude login`, or API key via
//! `ANTHROPIC_API_KEY`). We just detect whether the CLI is installed and
//! whether it appears to be configured.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::claude_mcp::{build_mcp_config, TempFile, ToolProfile};


/// Session id → pid of the running `claude` process. Lets us kill it later.
pub type Runners = Arc<Mutex<HashMap<String, u32>>>;

pub fn new_runners() -> Runners {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Spawn signature — the args that have to match for a warm CLI to be
/// reusable on the next `ask`. If any of these change between prewarm
/// and ask (user changed cwd, model, or workbench layout enough to
/// shift `appContext`), we kill the warm and respawn fresh.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SpawnSig {
    cwd: Option<PathBuf>,
    model: Option<String>,
    claude_uuid: String,
    resume: bool,
    rules_hash: u64,
    app_context_hash: u64,
    tool_profile: Option<String>,
}

fn hash_str(s: Option<&str>) -> u64 {
    let mut h = DefaultHasher::new();
    s.unwrap_or("").hash(&mut h);
    h.finish()
}

/// Per-session warm-pool of pre-spawned Claude CLIs. The actual
/// `WarmCli` struct is defined below alongside the spawn helpers; this
/// alias is hoisted so other modules can name the pool type without
/// forward-reference gymnastics. Pool size is implicitly bounded by
/// the number of active sessions; in practice only the focused chat
/// has a warm entry. Background sweeper evicts entries idle for
/// longer than `WARM_TTL_SECS`.
pub type WarmPool = Arc<tokio::sync::Mutex<HashMap<String, WarmCli>>>;

pub fn new_warm_pool() -> WarmPool {
    Arc::new(tokio::sync::Mutex::new(HashMap::new()))
}

/// How long a warm CLI is allowed to sit idle before the sweeper kills
/// it. Two-and-a-half minutes is comfortably longer than a typical
/// "type a message and click Send" window, but short enough that
/// abandoned spawns (user clicked away mid-typing) don't pile up.
const WARM_TTL_SECS: u64 = 150;

#[derive(Debug, Serialize, Clone)]
pub struct ClaudeStatus {
    /// `claude` binary found on PATH.
    pub detected: bool,
    /// Path to the binary if detected.
    pub path: Option<String>,
    /// Output of `claude --version`, trimmed.
    pub version: Option<String>,
    /// `~/.claude` dir exists — usually means `claude login` has been run,
    /// i.e. a Claude subscription session is configured.
    pub has_config_dir: bool,
    /// `ANTHROPIC_API_KEY` env var is set in our process env.
    pub has_api_key_env: bool,
    /// High-level bool for the UI: detected + (config dir or API key env var).
    pub ready: bool,
}

pub fn detect() -> ClaudeStatus {
    let path = which("claude");
    let detected = path.is_some();

    let version = path.as_ref().and_then(|p| read_version(p));
    let has_config_dir = home_dir()
        .map(|h| h.join(".claude").is_dir())
        .unwrap_or(false);
    let has_api_key_env = std::env::var("ANTHROPIC_API_KEY").is_ok();

    ClaudeStatus {
        detected,
        path: path.map(|p| p.display().to_string()),
        version,
        has_config_dir,
        has_api_key_env,
        ready: detected && (has_config_dir || has_api_key_env),
    }
}

fn which(name: &str) -> Option<PathBuf> {
    // Start with whatever PATH the process has (hydrated from the login shell
    // in `lib::run`), then fall back to the usual suspects in case the shell
    // hydration didn't cover them.
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(p) = std::env::var("PATH") {
        for dir in p.split(':') {
            if !dir.is_empty() {
                candidates.push(dir.to_string());
            }
        }
    }
    let fallbacks = ["/opt/homebrew/bin", "/usr/local/bin"];
    for extra in fallbacks {
        if !candidates.iter().any(|d| d == extra) {
            candidates.push(extra.into());
        }
    }
    if let Some(h) = home_dir() {
        for sub in [".local/bin", ".claude/local/bin", ".claude/local", ".bun/bin", ".volta/bin"] {
            let full = h.join(sub).to_string_lossy().into_owned();
            if !candidates.iter().any(|d| d == &full) {
                candidates.push(full);
            }
        }
    }
    for dir in candidates {
        let candidate = Path::new(&dir).join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(m) => m.is_file() && (m.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

fn read_version(path: &Path) -> Option<String> {
    let output = Command::new(path).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).to_string();
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// Minimal extension → media-type mapping for the four formats Anthropic's
/// vision endpoint accepts. Anything else falls through to `image/png` —
/// the API will reject unsupported types with a clear error rather than
/// silently corrupting the upload.
fn guess_image_media_type(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else {
        "image/png"
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClaudeRunError {
    #[error("claude CLI is not installed — run `curl -fsSL https://claude.ai/install.sh | bash` or see https://docs.claude.com/en/docs/claude-code/overview")]
    NotInstalled,
    #[error("claude is not authenticated — run `claude login` in your terminal or set ANTHROPIC_API_KEY")]
    NotAuthed,
    /// `--resume <uuid>` was passed but the Claude CLI couldn't locate that
    /// session in its on-disk store (`~/.claude/projects/...` — pruned, the
    /// CLI was reinstalled, or the user manually cleaned it). The frontend
    /// recovers by rotating the uuid, clearing `claudeResumable`, stamping a
    /// recap of the lost history into the next system prompt, and retrying
    /// once. Detected via best-effort phrase matching against the CLI's
    /// stderr — Anthropic doesn't publish a stable error code so the match
    /// is intentionally loose; false positives just trigger the same
    /// "session restarted with prior history baked in" recovery, which is
    /// the right outcome anyway.
    #[error("claude resume target is gone: {0}")]
    ResumeOrphan(String),
    #[error("claude failed: {0}")]
    Failed(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Heuristic stderr scan for "the uuid I tried to resume doesn't exist
/// anymore." Patterns broaden over time as we collect real-world failure
/// strings; for now this covers the obvious phrasings the CLI uses when
/// the session-id arg refers to nothing on disk.
fn looks_like_resume_orphan(stderr_tail: &str) -> bool {
    let t = stderr_tail.to_ascii_lowercase();
    // Negative-lookahead: filter out common non-orphan stderr that
    // happens to contain the bare word "session" + "not found". E.g.
    // `claude` complaining about a missing executable in PATH, or a
    // network error mentioning a not-found host. Without this guard
    // the loose pattern below was rotating uuids on transient errors
    // and breaking session continuity.
    let unrelated_phrases = [
        "command not found",
        "not found in path",
        "host not found",
        "permission denied",
        "ssl certificate",
        "no such file or directory",
        "repository not found",
        "branch not found",
    ];
    if unrelated_phrases.iter().any(|p| t.contains(p)) {
        return false;
    }
    // Require BOTH a session-context word AND an absence-phrase to
    // co-occur. A bare "not found" is too broad; bare "session" is
    // too broad. Their conjunction lands almost exclusively on the
    // stored-session-not-on-disk case we want to detect.
    let context_y = t.contains("session")
        || t.contains("conversation")
        || t.contains("resume target");
    let absence_y = t.contains("not found")
        || t.contains("no such")
        || t.contains("does not exist")
        || t.contains("doesn't exist")
        || t.contains("could not find")
        || t.contains("cannot find")
        || t.contains("unknown session")
        || t.contains("is not registered");
    (context_y && absence_y)
        // A few high-specificity standalone phrases — these alone
        // are enough because they're unambiguous about the cause.
        || t.contains("no resumable session")
        || t.contains("no conversation found")
        // "Session ID … is already in use": the previous CLI process
        // for this session was force-killed (idle-timeout shutdown,
        // SIGKILL on stop, OS reaped a wedged child). The session-id
        // lock file in the CLI's store is still there, so a fresh
        // spawn against the same uuid is rejected. Same self-heal
        // path as a real orphan — rotate uuid, recap, retry — fits
        // because the desired effect is identical: get the next turn
        // running with a clean CLI state. Only matches when "in use"
        // and "session" co-occur to keep the pattern from catching
        // unrelated lock-file errors.
        || (t.contains("is already in use") && t.contains("session"))
}

/// Args needed to spawn a `claude` CLI process. Threaded through the
/// cold-path `ask` and the warm-path `prewarm` together so the two
/// stay in lockstep — the warm pool's match check (see `SpawnSig`)
/// hashes the same fields the spawn actually uses.
#[derive(Debug, Clone)]
pub struct AskArgs<'a> {
    pub session_id: &'a str,
    pub cwd: Option<&'a Path>,
    pub claude_uuid: &'a str,
    pub resume: bool,
    pub rules: Option<&'a str>,
    pub model: Option<&'a str>,
    pub tool_profile: Option<&'a str>,
    pub app_context: Option<&'a str>,
    /// Path to the Tauri-side action-IPC Unix socket (passed via env
    /// to MCP sidecars so their `propose_*` tools can BLOCK on user
    /// approval). None when IPC is unavailable; sidecars then fall
    /// back to fire-and-forget card creation.
    pub action_ipc_socket: Option<&'a Path>,
}

fn build_spawn_sig(args: &AskArgs<'_>) -> SpawnSig {
    SpawnSig {
        cwd: args.cwd.map(|p| p.to_path_buf()),
        model: args.model.map(str::to_string),
        claude_uuid: args.claude_uuid.to_string(),
        resume: args.resume,
        rules_hash: hash_str(args.rules),
        app_context_hash: hash_str(args.app_context),
        tool_profile: args.tool_profile.map(str::to_string),
    }
}

/// One claude CLI process spawned with stream-json input mode and
/// pipes attached, blocked on stdin waiting for a user message.
/// `spawn_claude_armed` returns this; either `arm_streams` immediately
/// converts it for the cold-path consumer, or `prewarm` hands it off to
/// the warm pool for a future `ask` to pick up.
struct ArmedCli {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    stderr: tokio::process::ChildStderr,
    pid: u32,
    sig: SpawnSig,
    spawned_at: tokio::time::Instant,
    mcp_guard: Option<TempFile>,
}

/// Same set of pieces as `ArmedCli` but with stdout/stderr replaced by
/// background drain tasks that pump into an mpsc channel + a tail
/// buffer. We always wire these BEFORE storing in the warm pool —
/// otherwise the OS pipe buffer fills up with the CLI's startup
/// metadata (system event, init status, etc.) and the child blocks
/// before the user message even arrives, defeating the latency win.
struct StreamHandles {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
    stderr_buf: Arc<Mutex<String>>,
    drain_task: tokio::task::JoinHandle<()>,
    stderr_task: tokio::task::JoinHandle<()>,
    pid: u32,
    spawned_at: tokio::time::Instant,
    _mcp_guard: Option<TempFile>,
}

/// A pre-spawned claude CLI parked in `WarmPool`, paying its cold-start
/// cost while the user is still typing. Drain tasks keep stdout/stderr
/// flowing into channels so the CLI never blocks; a future `ask` with a
/// matching `SpawnSig` will pull this back out, write the user's
/// message to stdin, and stream the response.
pub struct WarmCli {
    sig: SpawnSig,
    handles: StreamHandles,
}

/// Spawn `claude` with stream-json input mode and pipes hooked up. The
/// process is alive but blocked on stdin — caller decides whether to
/// immediately write the user message (cold-path `ask`) or stash the
/// armed handle in the warm pool (`prewarm`). Universal stream-json
/// input is the key: it lets us decouple "spawn the CLI" from "pass
/// the prompt", which is what makes prewarm possible. Image-attachment
/// turns, text-only turns, and prewarm all share this one path now.
async fn spawn_claude_armed(args: &AskArgs<'_>) -> Result<ArmedCli, ClaudeRunError> {
    let status = detect();
    if !status.detected {
        return Err(ClaudeRunError::NotInstalled);
    }
    if !status.ready {
        return Err(ClaudeRunError::NotAuthed);
    }
    let bin = status.path.as_deref().unwrap_or("claude");

    let mcp = build_mcp_config(
        args.session_id,
        ToolProfile::from_str(args.tool_profile),
        args.action_ipc_socket,
    );
    let mcp_guard = mcp.as_ref().map(|(p, _)| TempFile(p.clone()));

    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("-p")
        .arg("--input-format")
        .arg("stream-json")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose");
    if args.resume {
        cmd.arg("--resume").arg(args.claude_uuid);
    } else {
        cmd.arg("--session-id").arg(args.claude_uuid);
    }
    if let Some(m) = args.model.map(str::trim).filter(|s| !s.is_empty()) {
        cmd.arg("--model").arg(m);
    }
    // Compose the system-prompt suffix in three parts. Order matters for
    // Claude's prompt-cache: the cache key is a prefix of the appended
    // text, so anything that's mostly-stable should come BEFORE anything
    // that mutates per turn.
    //   1. Memory hint — fully static.
    //   2. User rules — changes only when the user edits the Rules tab.
    //   3. Per-turn UI context — workbench layout, cwds, linked agents,
    //      one-shot cwd-switch recap. Goes LAST so the preceding block
    //      stays a stable prefix.
    let mut system_parts: Vec<String> = Vec::new();
    let memory_available = mcp
        .as_ref()
        .map(|(_, allowed)| {
            allowed.iter().any(|t| t.starts_with("mcp__memory__"))
        })
        .unwrap_or(false);
    if memory_available {
        system_parts.push(
            "Woom memory (persistent across sessions, ESSENTIAL — \
             the CLI session itself is ephemeral and may be wiped \
             between app restarts; this DB survives). Tools: \
             `mcp__memory__memory_search`, `memory_save`, `memory_list`, \
             `memory_get`, `memory_update`, `memory_delete`.\n\
             \n\
             SAVE aggressively, not reluctantly. The user's complaint \
             is that you don't save enough. Save by default; skip only \
             clearly-ephemeral state (in-flight task progress, raw \
             code, git output). Specifically save:\n\
             - User identity / role / stack / tooling preferences\n\
             - Project facts, naming, repo layout, key file paths the \
               user references repeatedly\n\
             - Workflow rules the user gives you ('always X', 'never Y', \
               'prefer Z over W'), with a brief 'why' line\n\
             - Decisions made this session that future sessions need \
               (architecture choices, tradeoffs accepted, deferred work)\n\
             - References to external systems (Linear/Jira project keys, \
               Slack channels, Grafana boards, dashboard URLs)\n\
             - Anything the user gets visibly frustrated about repeating\n\
             At minimum: if the user shares a fact you'd otherwise \
             ASK about next session, save it now. Better to over-save \
             trim-able rows than under-save and ask again.\n\
             \n\
             SEARCH at the start of every non-trivial turn — even \
             without obvious keywords. Use 2-5 word queries; FTS5 \
             with `unicode61` handles Russian + English the same way. \
             A miss is free; a missed save costs the user trust.\n\
             \n\
             Tag with `kind`: `user` / `feedback` / `project` / \
             `reference` / `note`. Keep entries terse (1-3 sentences). \
             Prefer `memory_update` over delete+save so ids stay \
             stable. If a recalled memory conflicts with current state, \
             trust what you see now and `memory_update` the stale row."
                .to_string(),
        );
    }
    // Don't-re-ask discipline. The most common UX complaint about
    // the agent is that it asks the user for data that ITSELF
    // discovered earlier in the same chat — project refs from a
    // prior `supabase projects list`, file paths from a prior
    // `find`, API hosts from a prior `curl`, etc. Re-asking reads
    // as you not paying attention.
    system_parts.push(
        "Don't re-ask for data that's already in this conversation. \
         Before asking the user for any concrete identifier — project \
         refs, repo names, file paths, API hosts, env values, ticket \
         IDs, branch names — scan your prior turns in this chat. If a \
         tool result, command output, or earlier message already \
         contains it (or contains enough to derive it: e.g. Supabase \
         project-ref → host `db.<ref>.supabase.co`), USE that, don't \
         ask. The user typed those facts (or you discovered them) \
         once; asking again wastes their time and erodes trust.\n\
         \n\
         Concrete shape: \"You showed me three project refs above; \
         I'll use <ref> for prod\" — not \"Can you give me the prod \
         project ref?\". When recap blocks (cwdSwitchRecap) appear \
         in your context, treat the data they carry as authoritative \
         — that's specifically there so you don't have to re-ask \
         after a Stop / restart."
            .to_string(),
    );
    // Action-chaining contract. Woom auto-resumes the agent's
    // turn after every approval card resolves (commit / PR / bash /
    // switch_cwd) — the result is fed back as a synthesised user
    // message and you immediately get a fresh turn to react. So the
    // anti-pattern is ending a turn with "ждy результата" / "waiting
    // for X to finish" — that text never reaches the user as a useful
    // signal because by the time they read it the next turn is already
    // streaming in. Just stop the turn after the propose_* call (or
    // chain another propose_* if the next step doesn't depend on the
    // outcome). Don't narrate "I'll wait for…".
    system_parts.push(
        "Action cards (`mcp__github__propose_commit` / `propose_pr` / \
         `propose_bash` / `propose_switch_cwd`): once you call one, \
         end the turn — Woom runs the card and AUTO-RESUMES you \
         with the outcome as the next turn (success or failure recap). \
         You do NOT need the user to type \"continue\" or \"go ahead\" \
         between cards. Don't write \"waiting for X\" / \"ждy коммит\" \
         filler — it's noise; just stop and the next turn arrives.\n\
         When a card fails, the failure summary lands as your next \
         turn's input — react and propose a fix (e.g. set upstream \
         branch, retry with --force-with-lease, etc) without asking \
         the user what to do unless you genuinely need a decision."
            .to_string(),
    );
    if let Some(r) = args.rules.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        system_parts.push(format!("User rules (follow these on every turn):\n\n{}", r));
    }
    if let Some(ctx) = args.app_context.map(str::trim).filter(|s| !s.is_empty()) {
        system_parts.push(ctx.to_string());
    }
    if !system_parts.is_empty() {
        cmd.arg("--append-system-prompt").arg(system_parts.join("\n\n---\n\n"));
    }
    if let Some((path, allowed)) = &mcp {
        cmd.arg("--mcp-config").arg(path);
        // Lock the CLI to ONLY our config — without this flag, Claude
        // also pulls in user-level MCP servers from `~/.claude.json`
        // and the bundled claude.ai connectors (Atlassian, Linear,
        // Notion, etc). That double-exposes Jira/etc as both
        // `mcp__jira__*` (ours, working) AND `createJiraIssue`/
        // `authenticate` (claude.ai's, needs OAuth) — the agent then
        // tells the user to OAuth into the claude.ai bundle while
        // ignoring our already-live connection. Strict mode keeps the
        // tool surface coherent.
        cmd.arg("--strict-mcp-config");
        if !allowed.is_empty() {
            cmd.arg("--allowedTools").arg(allowed.join(","));
        }
    }
    if let Some(dir) = args.cwd {
        cmd.current_dir(dir);
    }
    // Tauri-launched apps don't inherit shell PATH. Augment with common paths
    // beyond what `hydrate_path_from_login_shell` already set.
    if let Ok(p) = std::env::var("PATH") {
        let mut parts: Vec<String> = p.split(':').map(String::from).collect();
        for e in ["/opt/homebrew/bin", "/usr/local/bin"] {
            if !parts.iter().any(|d| d == e) {
                parts.push(e.into());
            }
        }
        if let Some(h) = home_dir() {
            for sub in [".local/bin", ".claude/local/bin", ".claude/local", ".bun/bin", ".volta/bin"] {
                let full = h.join(sub).to_string_lossy().into_owned();
                if !parts.iter().any(|d| d == &full) {
                    parts.push(full);
                }
            }
        }
        cmd.env("PATH", parts.join(":"));
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(std::process::Stdio::piped());

    let mut child = cmd.spawn()?;
    let pid = child.id().unwrap_or(0);
    let stdin = child.stdin.take().ok_or_else(|| ClaudeRunError::Failed("no stdin".into()))?;
    let stdout = child.stdout.take().ok_or_else(|| ClaudeRunError::Failed("no stdout".into()))?;
    let stderr = child.stderr.take().ok_or_else(|| ClaudeRunError::Failed("no stderr".into()))?;
    Ok(ArmedCli {
        child,
        stdin,
        stdout,
        stderr,
        pid,
        sig: build_spawn_sig(args),
        spawned_at: tokio::time::Instant::now(),
        mcp_guard,
    })
}

/// Wire stdout → mpsc + stderr → tail-buffer, returning a
/// `StreamHandles` ready for the consumer or for parking in the warm
/// pool. The drain tasks live for the rest of the CLI's lifetime; they
/// terminate naturally when the child closes its pipes on exit.
fn arm_streams(armed: ArmedCli) -> StreamHandles {
    use tokio::io::AsyncBufReadExt;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let stdout = armed.stdout;
    let drain_task = tokio::spawn(async move {
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let stderr_buf: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let stderr_buf_task = stderr_buf.clone();
    let stderr = armed.stderr;
    let stderr_task = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        const MAX_STDERR: usize = 16 * 1024;
        let mut reader = tokio::io::BufReader::new(stderr);
        let mut chunk = [0u8; 2048];
        loop {
            match reader.read(&mut chunk).await {
                Ok(0) => break,
                Ok(n) => {
                    let s = String::from_utf8_lossy(&chunk[..n]);
                    let mut buf = stderr_buf_task.lock().unwrap();
                    buf.push_str(&s);
                    if buf.len() > MAX_STDERR {
                        // Keep only the tail — that's where the actual error
                        // typically lives when the CLI fails.
                        let start = buf.len() - MAX_STDERR;
                        *buf = buf[start..].to_string();
                    }
                }
                Err(_) => break,
            }
        }
    });
    StreamHandles {
        child: armed.child,
        stdin: armed.stdin,
        stdout_rx: rx,
        stderr_buf,
        drain_task,
        stderr_task,
        pid: armed.pid,
        spawned_at: armed.spawned_at,
        _mcp_guard: armed.mcp_guard,
    }
}

/// Run `claude` and stream JSONL events to the frontend via
/// `claude:stream:{session_id}`. Tries the warm pool first — if a
/// prewarmed CLI is sitting around with a matching spawn signature
/// (cwd / model / uuid / resume / rules / appContext / tool profile),
/// we reuse it and skip the cold-start cost (binary load + `--resume`
/// history hydration). Otherwise we spawn fresh.
///
/// `claude_uuid` is the Claude CLI session UUID (independent of
/// Woom's own `session_id`). `resume=true` → `--resume <uuid>`,
/// `resume=false` → `--session-id <uuid>` (creates a fresh persisted
/// session). On `ResumeOrphan` the frontend self-heals with a new
/// uuid + recap injection.
pub async fn ask(
    app: tauri::AppHandle,
    runners: Runners,
    warm_pool: WarmPool,
    session_id: &str,
    prompt: &str,
    cwd: Option<&Path>,
    claude_uuid: &str,
    resume: bool,
    rules: Option<&str>,
    // Forwarded as `--model <id>`. None → no flag, CLI picks default.
    model: Option<&str>,
    // Tool profile name: 'coding' / 'triage' / 'pr-review' / 'all'.
    tool_profile: Option<&str>,
    app_context: Option<&str>,
    action_ipc_socket: Option<&Path>,
    image_paths: &[String],
) -> Result<String, ClaudeRunError> {
    let args = AskArgs {
        session_id, cwd, claude_uuid, resume, rules, model, tool_profile, app_context,
        action_ipc_socket,
    };
    let target_sig = build_spawn_sig(&args);

    // Take any warm entry for this session. If the signature matches
    // we'll reuse it; otherwise it's stale (user changed cwd / model /
    // typed in a different chat / etc.) and we kill it before
    // spawning fresh, so the next prewarm starts clean.
    let warm = {
        let mut pool = warm_pool.lock().await;
        pool.remove(session_id)
    };

    let handles = match warm {
        Some(w) if w.sig == target_sig => w.handles,
        Some(stale) => {
            kill_warm_handles(stale.handles).await;
            arm_streams(spawn_claude_armed(&args).await?)
        }
        None => arm_streams(spawn_claude_armed(&args).await?),
    };

    consume_handles(app, runners, handles, session_id, prompt, image_paths).await
}

/// Write the user's message + image content blocks to stdin, drain
/// stdout to the frontend stream channel, reap the child, and surface
/// an orphan-or-real-error result. Shared by cold + warm paths.
async fn consume_handles(
    app: tauri::AppHandle,
    runners: Runners,
    handles: StreamHandles,
    session_id: &str,
    prompt: &str,
    image_paths: &[String],
) -> Result<String, ClaudeRunError> {
    use tauri::Emitter;
    use tokio::io::AsyncWriteExt;

    let StreamHandles {
        mut child,
        mut stdin,
        mut stdout_rx,
        stderr_buf,
        drain_task,
        stderr_task,
        pid,
        spawned_at: _,
        _mcp_guard,
    } = handles;

    if pid != 0 {
        runners.lock().unwrap().insert(session_id.to_string(), pid);
    }

    // Build the user message and write it to stdin. Same Anthropic
    // API content-block shape as before — text first, then any image
    // blocks. Empty `image_paths` just means a plain text turn.
    //
    // Skip the text block entirely when prompt is empty/whitespace.
    // Anthropic's API rejects `{"type":"text","text":""}` with a 400
    // "messages: text content blocks must be non-empty"; the most
    // common trigger was a user dropping a screenshot in with no
    // accompanying text — UI accepts it, we'd push an empty text
    // block, the request 400s, and the chat shows nothing but the
    // raw API error. When prompt is empty AND there are no images
    // either, we still need ONE block — fall back to a single space
    // so the message round-trips (the model sees effectively no
    // input but the API contract holds).
    {
        use base64::Engine;
        let mut content: Vec<serde_json::Value> = Vec::new();
        let prompt_trimmed = prompt.trim();
        if !prompt_trimmed.is_empty() {
            content.push(serde_json::json!({ "type": "text", "text": prompt }));
        }
        for p in image_paths {
            let bytes = match tokio::fs::read(p).await {
                Ok(b) => b,
                Err(_) => continue,
            };
            let media_type = guess_image_media_type(p);
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            content.push(serde_json::json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": media_type,
                    "data": b64,
                }
            }));
        }
        // Fallback: every degenerate path (empty prompt + zero
        // images, or every image read failing) leaves `content`
        // empty. The API rejects an empty content array with the
        // same 400 we're trying to avoid — guarantee at least one
        // block. A single space round-trips cleanly and the model
        // treats it as an effectively empty turn.
        if content.is_empty() {
            content.push(serde_json::json!({ "type": "text", "text": " " }));
        }
        let msg = serde_json::json!({
            "type": "user",
            "message": { "role": "user", "content": content }
        });
        let line = msg.to_string();
        let _ = stdin.write_all(line.as_bytes()).await;
        let _ = stdin.write_all(b"\n").await;
        let _ = stdin.shutdown().await;
    }

    let event_name = format!("claude:stream:{}", session_id);
    let mut final_text = String::new();
    let mut got_result = false;
    let mut forced_kill = false;

    // Drain stdout with two timing bounds:
    //   - After we've seen the `result` event, the turn is logically
    //     done — `final_text` is captured. We give the CLI 2s to
    //     emit any trailing events (usage stats, system summary) and
    //     close stdout cleanly; past that we force-kill so the UI's
    //     "thinking" indicator can drop. The forced_kill path returns
    //     Ok(final_text) anyway (see below), so the kill is purely
    //     a cleanup expedient — not an error condition. Was 30s,
    //     which made the UI animate the thinking dots for 15-20s
    //     after every turn while the CLI tore down its 5 MCP
    //     sidecars; users read that as "Woom is hung". 2s
    //     covers the common trailing-events window without making
    //     the CLI's MCP-shutdown latency user-visible.
    //   - With no `result` yet, 15 minutes idle is the hard ceiling — long
    //     Claude turns can legitimately run 5-8 min, but not 15. Past that,
    //     something's wedged and we'd rather fail fast than show an
    //     indefinite spinner.
    // Either timeout path force-kills + breaks; the recv loop never hangs
    // forever after this. Without these bounds, a single stuck CLI was
    // enough to wedge the chat tab until the user manually killed the
    // process from a terminal.
    loop {
        let timeout_dur = if got_result {
            std::time::Duration::from_secs(2)
        } else {
            std::time::Duration::from_secs(900)
        };
        let line = match tokio::time::timeout(timeout_dur, stdout_rx.recv()).await {
            Ok(Some(line)) => line,
            Ok(None) => break, // drain task ended (child exited, normal path)
            Err(_) => {
                // Timeout. The CLI isn't producing output — kill it so
                // the rest of this function unwinds cleanly.
                if pid > 0 {
                    unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                }
                forced_kill = true;
                break;
            }
        };
        let _ = app.emit(&event_name, &line);
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            if v.get("type").and_then(|t| t.as_str()) == Some("result") {
                got_result = true;
                if let Some(r) = v.get("result").and_then(|r| r.as_str()) {
                    final_text = r.to_string();
                }
            }
        }
    }

    // Bounded reap. If the recv loop exited normally (drain task ended
    // because child closed stdout), `child.wait()` returns the exit
    // status immediately. If we forced a kill, the child is in the
    // process of dying and wait() should return within milliseconds.
    // Either way, 3 seconds is more than enough — and capping it
    // prevents this function from hanging if the kernel takes its
    // sweet time reaping a wedged child.
    let out = match tokio::time::timeout(
        std::time::Duration::from_secs(3),
        child.wait(),
    ).await {
        Ok(o) => o,
        Err(_) => {
            // Wait timed out — last-resort SIGKILL + brief retry.
            if pid > 0 {
                unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            }
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                child.wait(),
            ).await.unwrap_or_else(|_| {
                // Give up reaping. The process record stays as a
                // zombie until OS cleanup; we synthesize a fake
                // "killed" status so the rest of this function can
                // unwind. The frontend will surface the timeout.
                Ok(std::process::ExitStatus::from_raw(libc::SIGKILL))
            })
        }
    };
    // Await both background tasks so neither lingers past this
    // function's lifetime. Each ends naturally when its pipe closes
    // on child exit; awaiting after `child.wait()` returned is just a
    // formality unless something pathological held the pipe open. The
    // join handles still need to be consumed — without these awaits a
    // pathological CLI that refused to exit cleanly could leave the
    // tokio runtime tracking forgotten tasks indefinitely.
    let _ = stderr_task.await;
    let _ = drain_task.await;
    runners.lock().unwrap().remove(session_id);
    let out = out?;

    if let Some(code) = out.code() {
        if code == 143 {
            return Err(ClaudeRunError::Failed("cancelled".into()));
        }
    }
    if forced_kill {
        // We killed the CLI because it stopped producing output. If we
        // already saw a `result` event, the turn is logically complete
        // — the kill just shortcut a slow MCP-sidecar shutdown. Empty
        // `final_text` is FINE in that case: it just means the agent
        // ended its turn with a tool call (e.g. propose_bash) and no
        // follow-up text, which is exactly what we want it to do.
        // The actual streamed assistant text already landed in the UI
        // via `claude:stream:<id>` events during the recv loop, so
        // returning Ok("") here doesn't blank the bubble — it just
        // tells the caller "turn ended cleanly". Pre-result kills
        // still surface as the original error so the UI can recover.
        if got_result {
            return Ok(final_text);
        }
        return Err(ClaudeRunError::Failed(
            "claude CLI stopped responding (no usable output for an extended period); forced shutdown."
                .into(),
        ));
    }
    if !out.success() {
        let stderr_tail = stderr_buf.lock().unwrap().trim().to_string();
        let code = out.code().unwrap_or(-1);
        if looks_like_resume_orphan(&stderr_tail) {
            return Err(ClaudeRunError::ResumeOrphan(stderr_tail));
        }
        let msg = if stderr_tail.is_empty() {
            format!("exit code {code}")
        } else {
            format!("exit code {code} — {stderr_tail}")
        };
        return Err(ClaudeRunError::Failed(msg));
    }
    Ok(final_text)
}

/// Per-session "prewarm currently in flight" guard. Sits beside the
/// async warm pool because this guard needs cheap synchronous
/// take-or-skip semantics, which `tokio::sync::Mutex` doesn't give
/// (it's only async-lockable). Without this, two concurrent prewarms
/// for the same session both pass the fast-path check, both evict,
/// both spawn, both insert — last write wins and the first CLI
/// process leaks (TTL sweeper picks it up in 150s, but until then
/// it's ~500MB of orphaned RAM). The race is rare in practice
/// because frontend debounces prewarm by 250ms, but a slow spawn
/// (>250ms) opens the window.
static PREWARM_IN_FLIGHT: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::OnceLock::new();

fn prewarm_in_flight() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    PREWARM_IN_FLIGHT.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// RAII guard that clears the in-flight bit on drop, regardless of
/// the prewarm path's exit (success / spawn-error / panic). Keeping
/// the cleanup tied to scope drop instead of an explicit branch
/// means future code edits can't accidentally bypass it.
struct PrewarmInFlightGuard {
    session_id: String,
}

impl Drop for PrewarmInFlightGuard {
    fn drop(&mut self) {
        if let Ok(mut g) = prewarm_in_flight().lock() {
            g.remove(&self.session_id);
        }
    }
}

/// Pre-spawn a `claude` CLI for `session_id` and park it in the warm
/// pool with the given spawn args. If a prior warm entry exists with
/// the same signature, this is a cheap no-op (the existing armed
/// process is still good). If signatures differ, the stale entry is
/// killed first. The next `ask` for this session with matching args
/// will pick up the parked CLI and skip the cold-start cost.
pub async fn prewarm(
    warm_pool: WarmPool,
    session_id: &str,
    cwd: Option<&Path>,
    claude_uuid: &str,
    resume: bool,
    rules: Option<&str>,
    model: Option<&str>,
    tool_profile: Option<&str>,
    app_context: Option<&str>,
    action_ipc_socket: Option<&Path>,
) -> Result<(), ClaudeRunError> {
    // Race-free single-prewarm-per-session guard. If another prewarm
    // for this same session is still in flight, this call is a
    // no-op — the in-flight one will land its result and the eventual
    // `ask` will pick it up. Skipping is the correct behavior: the
    // frontend's prewarm calls are idempotent best-effort, not
    // ordered; the agent doesn't care which one's spawn lands first.
    {
        let mut g = prewarm_in_flight()
            .lock()
            .expect("prewarm_in_flight mutex poisoned");
        if !g.insert(session_id.to_string()) {
            return Ok(());
        }
    }
    let _guard = PrewarmInFlightGuard {
        session_id: session_id.to_string(),
    };

    let args = AskArgs {
        session_id, cwd, claude_uuid, resume, rules, model, tool_profile, app_context,
        action_ipc_socket,
    };
    let target_sig = build_spawn_sig(&args);

    // Fast path: existing warm entry already matches → leave it alone.
    {
        let pool = warm_pool.lock().await;
        if let Some(existing) = pool.get(session_id) {
            if existing.sig == target_sig {
                return Ok(());
            }
        }
    }

    // Single-warm policy: pool size is capped at one entry across the
    // whole app. Each warm CLI holds the `claude` process plus a fan
    // of MCP sidecars (~200-500 MB total per session), so letting them
    // accumulate when a user clicks through several chats in quick
    // succession would chew real resources for spawns the user is
    // unlikely to actually consume. Evict everyone else for this
    // session AND every other session — the last-touched chat wins,
    // the rest take the cold-start hit on their next ask.
    //
    // Drains lock then kills outside it: `kill_warm_handles` awaits
    // child.wait() which can take up to 1500ms, and we don't want to
    // block other prewarm/drop callers behind that.
    let evicted: Vec<WarmCli> = {
        let mut pool = warm_pool.lock().await;
        let keys: Vec<String> = pool.keys().cloned().collect();
        let mut taken = Vec::with_capacity(keys.len());
        for k in keys {
            if let Some(w) = pool.remove(&k) {
                taken.push(w);
            }
        }
        taken
    };
    for w in evicted {
        kill_warm_handles(w.handles).await;
    }

    let armed = spawn_claude_armed(&args).await?;
    let sig = armed.sig.clone();
    let handles = arm_streams(armed);
    let mut pool = warm_pool.lock().await;
    pool.insert(
        session_id.to_string(),
        WarmCli { sig, handles },
    );
    Ok(())
}

/// Drop any warm CLI parked for this session. Safe to call when there
/// isn't one — returns Ok in that case. Used by the frontend on tab
/// switch / cwd change / "user gave up typing", and by the TTL
/// sweeper.
pub async fn drop_prewarm(warm_pool: WarmPool, session_id: &str) {
    let entry = {
        let mut pool = warm_pool.lock().await;
        pool.remove(session_id)
    };
    if let Some(w) = entry {
        kill_warm_handles(w.handles).await;
    }
}

/// SIGTERM → wait → SIGKILL the underlying process and reap it.
/// Drain tasks terminate naturally when the pipes close on child
/// exit, so we don't need to abort them explicitly — but we do
/// `await` the child so we don't leak a zombie.
///
/// Without the SIGKILL fallback, a CLI that's blocked in a syscall
/// (network read, MCP tool stuck, etc.) silently ignores SIGTERM
/// and stays alive — which means it keeps holding the Claude CLI's
/// session-uuid lock. The next `--resume <uuid>` ask then errors
/// out with "Session ID is already in use", stranding the chat
/// until the user manually kills the process. Real-world repro:
/// observed PIDs alive 2 hours after Stop with 4 sec CPU time.
async fn kill_warm_handles(mut handles: StreamHandles) {
    if handles.pid > 0 {
        unsafe { libc::kill(handles.pid as i32, libc::SIGTERM) };
    }
    // Drop stdin first so the CLI doesn't hang waiting on input that's
    // never coming. Then wait — short timeout so a stuck child can't
    // wedge the sweeper.
    drop(handles.stdin);
    let timeout_result = tokio::time::timeout(
        std::time::Duration::from_millis(1500),
        handles.child.wait(),
    )
    .await;
    if timeout_result.is_err() && handles.pid > 0 {
        // SIGTERM didn't take. Force-kill — the process is wedged in
        // a syscall and will not reach the signal handler until
        // something interrupts it. SIGKILL goes straight through.
        unsafe { libc::kill(handles.pid as i32, libc::SIGKILL) };
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(800),
            handles.child.wait(),
        )
        .await;
    }
    let _ = handles.stderr_task.await;
}

/// Sweep the warm pool: any entry idle longer than `WARM_TTL_SECS`
/// gets killed. Called from a Tauri background task on a timer. Keeps
/// abandoned prewarms (user typed then closed the tab without sending)
/// from accumulating CLI processes indefinitely.
///
/// Race-safe against concurrent prewarms: the sweeper collects
/// `(session_id, spawned_at)` pairs under the lock, then between the
/// release-and-kill phases checks that the pool's CURRENT entry for
/// each session still has the same `spawned_at`. If a fresh prewarm
/// landed in the meantime, its `spawned_at` is newer than the one we
/// captured and we leave it alone. Without this guard the sweeper
/// could occasionally kill a 1-second-old freshly-prewarmed CLI
/// because it raced with the eviction-decision phase.
pub async fn evict_stale_warm(warm_pool: WarmPool) {
    let now = tokio::time::Instant::now();
    let ttl = std::time::Duration::from_secs(WARM_TTL_SECS);
    let candidates: Vec<(String, tokio::time::Instant)> = {
        let pool = warm_pool.lock().await;
        pool.iter()
            .filter_map(|(k, v)| {
                if now.duration_since(v.handles.spawned_at) >= ttl {
                    Some((k.clone(), v.handles.spawned_at))
                } else {
                    None
                }
            })
            .collect()
    };
    for (id, captured_spawned_at) in candidates {
        // Re-acquire lock and verify the entry STILL matches what we
        // collected. A fresh prewarm replaces the entry under the same
        // session id with a NEW spawned_at, so a mismatch means the
        // entry is no longer our target — leave it alone, the sweeper
        // will revisit on the next tick if the new entry also ages out.
        let still_stale = {
            let pool = warm_pool.lock().await;
            pool.get(&id)
                .map(|v| v.handles.spawned_at == captured_spawned_at)
                .unwrap_or(false)
        };
        if still_stale {
            drop_prewarm(warm_pool.clone(), &id).await;
        }
    }
}

/// One-off headless Claude call: feed it the staged git diff from `repo`
/// and return a single-line commit message. Separate from the chat-session
/// `ask()` path — uses a throwaway session so it never lands in the user's
/// chat history or the active linked-column transcript.
pub async fn generate_commit_message(repo: &std::path::Path) -> Result<String, ClaudeRunError> {
    let status = detect();
    if !status.detected {
        return Err(ClaudeRunError::NotInstalled);
    }
    if !status.ready {
        return Err(ClaudeRunError::NotAuthed);
    }
    let bin = status.path.as_deref().unwrap_or("claude");

    // Pull the staged diff. If nothing's staged, caller asked for a commit
    // message out of nothing — surface that explicitly.
    let diff_out = tokio::process::Command::new("git")
        .current_dir(repo)
        .args(["diff", "--cached"])
        .output()
        .await?;
    if !diff_out.status.success() {
        let stderr = String::from_utf8_lossy(&diff_out.stderr).trim().to_string();
        return Err(ClaudeRunError::Failed(format!(
            "git diff --cached failed: {}",
            if stderr.is_empty() { "unknown error".into() } else { stderr }
        )));
    }
    let diff = String::from_utf8_lossy(&diff_out.stdout);
    if diff.trim().is_empty() {
        return Err(ClaudeRunError::Failed(
            "Nothing is staged — stage at least one change before asking for a commit message.".into(),
        ));
    }

    // Tight prompt — we want just the subject line, no chatter.
    let prompt = format!(
        "Write a single git commit message for the following staged diff.\n\n\
         Rules:\n\
         - Imperative mood (\"Add X\", \"Fix Y\", not \"Added X\").\n\
         - Under 72 characters.\n\
         - No quotes, no markdown, no preamble, no explanation.\n\
         - Output ONLY the commit message on a single line.\n\n\
         ```diff\n{diff}```"
    );

    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("-p").arg(&prompt);
    cmd.current_dir(repo);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    // Mirror the PATH augmentation from `ask()` — Tauri-launched processes
    // inherit a skinny PATH, and the same `claude` binary lookup pattern
    // applies here.
    if let Ok(p) = std::env::var("PATH") {
        let mut parts: Vec<String> = p.split(':').map(String::from).collect();
        for e in ["/opt/homebrew/bin", "/usr/local/bin"] {
            if !parts.iter().any(|d| d == e) {
                parts.push(e.into());
            }
        }
        if let Some(h) = home_dir() {
            for sub in [".local/bin", ".claude/local/bin", ".claude/local", ".bun/bin", ".volta/bin"] {
                let full = h.join(sub).to_string_lossy().into_owned();
                if !parts.iter().any(|d| d == &full) {
                    parts.push(full);
                }
            }
        }
        cmd.env("PATH", parts.join(":"));
    }

    let out = cmd.output().await?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let code = out.status.code().unwrap_or(-1);
        return Err(ClaudeRunError::Failed(format!(
            "claude exited {code}{}",
            if stderr.is_empty() { String::new() } else { format!(" — {stderr}") }
        )));
    }
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // Strip common wrappers the model sometimes adds even when instructed
    // otherwise (backticks, quotes, a trailing period run-on). Keep it
    // single-line — the commit-message field is one-line.
    let cleaned = raw
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'' || c == '`')
        .to_string();
    if cleaned.is_empty() {
        return Err(ClaudeRunError::Failed("claude returned an empty response".into()));
    }
    Ok(cleaned)
}

/// Result of a successful compact-fork. The frontend swaps the
/// session's `claudeUuid` to `new_uuid` and appends a system message
/// to the chat carrying `summary` so the user can read what the new
/// session was seeded with.
#[derive(Debug, serde::Serialize, Clone)]
pub struct CompactResult {
    pub new_uuid: String,
    pub summary: String,
}

/// Two-shot fork-compact: ask the existing CLI session to summarise
/// itself, then seed a brand-new session with that summary as the
/// first turn. The summary call uses `--resume <old_uuid>` so the
/// model has full prior context; the seed call uses `--session-id
/// <new_uuid>` and a short ack-prompt so the new session has stored
/// history starting from the summary, ready for `--resume <new_uuid>`
/// on the user's very next normal turn.
///
/// We bypass `ask()` here so the calls don't emit on the
/// `claude:stream:<session_id>` channel (the chat would briefly show
/// a wall of summary text mid-compact otherwise). Instead each call
/// runs as a plain `claude -p <prompt> --output-format json`,
/// blocking until done.
pub async fn compact_session(
    old_uuid: &str,
    new_uuid: &str,
    cwd: Option<&Path>,
    model: Option<&str>,
) -> Result<CompactResult, ClaudeRunError> {
    let status = detect();
    if !status.detected {
        return Err(ClaudeRunError::NotInstalled);
    }
    if !status.ready {
        return Err(ClaudeRunError::NotAuthed);
    }
    let bin = status.path.as_deref().unwrap_or("claude");

    // Step 1: summary. Tight prompt so the response is focused — the
    // sentinel header lets us strip any pre-amble Claude sometimes
    // emits anyway despite "no preamble" instructions.
    let summary_prompt = "Output ONLY a concise (300-500 word) summary of this conversation \
        so far — what was asked, what was decided, what was done, what's still in flight, \
        and any code/config decisions worth remembering. No preamble, no sign-off, no \
        meta-commentary about \"summarising\" — just the summary content.";
    let summary = run_claude_oneshot(bin, summary_prompt, Some(old_uuid), None, cwd, model).await?;
    let summary = summary.trim();
    if summary.is_empty() {
        return Err(ClaudeRunError::Failed(
            "claude returned an empty summary — old session may not be resumable".into(),
        ));
    }

    // Step 2: seed new session. Keep the prompt small (no tool use, no
    // navigation) — it just needs to land enough turn history that the
    // next `--resume <new_uuid>` works. Ack body is intentionally
    // discarded; we keep `summary` as the user-facing artifact.
    let seed_prompt = format!(
        "This is a continuation of an earlier Claude Code session. \
         The prior conversation has been compacted into the summary below. \
         Reply with a brief one-sentence acknowledgement (e.g. \"Ready to continue.\") \
         — the user's next message will pick up from here.\n\n\
         === PRIOR-SESSION SUMMARY ===\n{summary}\n=== END SUMMARY ==="
    );
    let _ack = run_claude_oneshot(bin, &seed_prompt, None, Some(new_uuid), cwd, model).await?;

    Ok(CompactResult {
        new_uuid: new_uuid.to_string(),
        summary: summary.to_string(),
    })
}

/// Run a single non-streaming `claude -p` call and return the
/// `result` field from the JSON output. Used by `compact_session` for
/// both the summary call and the seed call. No MCP, no
/// `--append-system-prompt`, no images — just the prompt text and
/// (optionally) `--resume` / `--session-id` / `--model`.
async fn run_claude_oneshot(
    bin: &str,
    prompt: &str,
    resume_uuid: Option<&str>,
    new_session_uuid: Option<&str>,
    cwd: Option<&Path>,
    model: Option<&str>,
) -> Result<String, ClaudeRunError> {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("-p").arg(prompt).arg("--output-format").arg("json");
    if let Some(u) = resume_uuid {
        cmd.arg("--resume").arg(u);
    } else if let Some(u) = new_session_uuid {
        cmd.arg("--session-id").arg(u);
    }
    if let Some(m) = model.map(str::trim).filter(|s| !s.is_empty()) {
        cmd.arg("--model").arg(m);
    }
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    // Same PATH augmentation as `ask()` / `generate_commit_message` —
    // Tauri-launched processes inherit a skinny PATH, and `claude` may
    // live in `~/.claude/local/bin` etc. depending on install method.
    if let Ok(p) = std::env::var("PATH") {
        let mut parts: Vec<String> = p.split(':').map(String::from).collect();
        for e in ["/opt/homebrew/bin", "/usr/local/bin"] {
            if !parts.iter().any(|d| d == e) {
                parts.push(e.into());
            }
        }
        if let Some(h) = home_dir() {
            for sub in [".local/bin", ".claude/local/bin", ".claude/local", ".bun/bin", ".volta/bin"] {
                let full = h.join(sub).to_string_lossy().into_owned();
                if !parts.iter().any(|d| d == &full) {
                    parts.push(full);
                }
            }
        }
        cmd.env("PATH", parts.join(":"));
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let out = cmd.output().await?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let code = out.status.code().unwrap_or(-1);
        return Err(ClaudeRunError::Failed(format!(
            "claude exited {code}{}",
            if stderr.is_empty() { String::new() } else { format!(" — {stderr}") }
        )));
    }
    // `--output-format json` returns one envelope: `{"result": "...",
    // "session_id": "...", "is_error": false, ...}`. Pull `result`,
    // tolerate missing-field as an empty string so the caller surfaces
    // a "claude returned empty" instead of a JSON-parse error.
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| ClaudeRunError::Failed(format!("claude json parse failed: {e}")))?;
    let result = parsed
        .get("result")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Ok(result)
}

// MCP-config plumbing (ToolProfile, build_mcp_config, per-server
// builders, sidecar discovery, TempFile guard) lives in `claude_mcp.rs`
// — claude.rs just imports and uses them via `build_mcp_config(...)`
// up top.


/// Send SIGTERM to a running claude spawn for the given session,
/// then SIGKILL after a short grace period if it didn't take.
/// Background task does the escalation so this stays a sync entry
/// point (it's wired through Tauri's IPC, which doesn't await).
///
/// SIGKILL fallback is critical: when the CLI is blocked in a
/// syscall (network, MCP-tool RPC, file IO), SIGTERM is queued but
/// never delivered until the syscall returns. SIGKILL goes through
/// the kernel directly. Without it, a "Stop"-clicked session can
/// stay alive indefinitely, holding its session-uuid lock and
/// blocking the next `--resume` with "Session ID is already in use".
pub fn stop(runners: &Runners, session_id: &str) -> bool {
    let pid = runners.lock().unwrap().get(session_id).copied();
    match pid {
        Some(pid) if pid > 0 => {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            // Use Tauri's async runtime — `claude_stop` is a sync Tauri
            // command, so calling `tokio::spawn` directly here panics
            // ("there is no reactor running"). `tauri::async_runtime::spawn`
            // is reactor-agnostic: it picks up Tauri's global runtime
            // regardless of which thread invokes it.
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                // signal 0 just probes existence — returns 0 if alive,
                // -1 if dead/reaped. PID reuse risk in a 2-second
                // window is negligible.
                if unsafe { libc::kill(pid as i32, 0) } == 0 {
                    unsafe {
                        libc::kill(pid as i32, libc::SIGKILL);
                    }
                }
            });
            true
        }
        _ => false,
    }
}
