//! Skills — user-defined slash commands that expand into a fully
//! pre-resolved prompt (Claude Code's signature trick — see
//! `docs/CLAUDE_PARITY.md §3`). A skill lives at:
//!
//!   - `~/.claude/skills/<name>/SKILL.md`           (user-global)
//!   - `<repo>/.claude/skills/<name>/SKILL.md`      (project-scoped,
//!                                                  walked up from cwd)
//!
//! SKILL.md is a markdown doc with optional YAML frontmatter. The body
//! supports two substitutions before being injected as a user message:
//!
//!   1. `$ARGUMENTS`   → user's text after the slash command
//!   2. `` !`<cmd>` `` → stdout of running `<cmd>` in `sh -c` from the
//!                       skill's cwd. 30s per-command timeout, 100 KB
//!                       output cap. Failures inject a fenced
//!                       `[shell-error: …]` block so the agent sees
//!                       what went wrong instead of silently missing
//!                       context. Multi-line ```\u{60}\u{60}\u{60}!``
//!                       blocks are NOT yet supported — single-line
//!                       backticked command only.
//!
//! Frontmatter is a tiny hand-rolled parser (no yaml-rust dep) that
//! handles `key: value`, `key: "value"`, and `key: [a, b, c]`. Lines
//! starting with `#` are comments. Unknown keys are preserved
//! verbatim so a user can hand-edit forward-compatible fields.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs::bash_run;

/// Bundled defaults — written into `~/.claude/skills/<name>/SKILL.md`
/// on first launch if the file isn't there yet. Files live under
/// `apps/desktop/src-tauri/assets/skills/` in the repo. `include_str!`
/// inlines them into the binary so no runtime resource resolution is
/// needed.
const BUNDLED_REVIEW_PR: &str = include_str!("../assets/skills/review-pr/SKILL.md");
const BUNDLED_SUMMARIZE_CHANGES: &str =
    include_str!("../assets/skills/summarize-changes/SKILL.md");
const BUNDLED_EXPLORE_REPO: &str = include_str!("../assets/skills/explore-repo/SKILL.md");

const BUNDLED_DEFAULTS: &[(&str, &str)] = &[
    ("review-pr", BUNDLED_REVIEW_PR),
    ("summarize-changes", BUNDLED_SUMMARIZE_CHANGES),
    ("explore-repo", BUNDLED_EXPLORE_REPO),
];

/// Idempotent — drops each bundled SKILL.md into `~/.claude/skills/`
/// unless the file already exists. Never overwrites the user's edits.
/// Returns the count of files actually written (0 = no-op, already
/// installed on a previous run).
pub fn install_bundled_defaults() -> usize {
    let Some(home) = dirs_next_home() else { return 0 };
    let root = home.join(".claude").join("skills");
    let mut written = 0;
    for (name, body) in BUNDLED_DEFAULTS {
        let dir = root.join(name);
        let path = dir.join("SKILL.md");
        if path.exists() {
            continue;
        }
        if std::fs::create_dir_all(&dir).is_err() {
            continue;
        }
        if std::fs::write(&path, body).is_ok() {
            written += 1;
        }
    }
    written
}

/// Surface a skill at the SKILL.md path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Globally-unique id. Format: `<scope>:<name>` where scope is
    /// `user` for `~/.claude/skills/...` or `project` for
    /// `<repo>/.claude/skills/...`. Lets the frontend disambiguate
    /// same-named skills across scopes.
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub when_to_use: Option<String>,
    pub argument_hint: Option<String>,
    /// Tool names allowed for the turn that triggered the skill. We
    /// don't yet narrow the agent's tool allowlist on dispatch (would
    /// require threading through `claude_mcp.rs`) — recorded for the
    /// follow-up phase.
    pub allowed_tools: Vec<String>,
    /// Optional model override for the skill turn. Same caveat as
    /// `allowed_tools` — recorded but not yet applied.
    pub model: Option<String>,
    pub scope: SkillScope,
    pub path: String,
    /// Raw body (with frontmatter stripped). Substitutions run at
    /// render time, not discover time, so per-call args / shell
    /// outputs stay fresh.
    #[serde(skip)]
    pub body: String,
    /// Any frontmatter keys we don't model — kept so the frontend can
    /// surface them in a "raw" view if needed.
    #[serde(default)]
    pub extras: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SkillScope {
    User,
    Project,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderedSkill {
    pub skill: Skill,
    pub rendered: String,
    /// Per-command outcomes — handy for the UI to surface "shell
    /// resolution: 3 ok, 1 error" inline if the user wants to debug.
    pub shell_results: Vec<ShellResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellResult {
    pub cmd: String,
    pub ok: bool,
    pub code: i32,
    pub stdout_truncated: bool,
}

// ---- Discovery -----------------------------------------------------------

pub fn user_skills_dir() -> Option<PathBuf> {
    dirs_next_home().map(|h| h.join(".claude").join("skills"))
}

fn dirs_next_home() -> Option<PathBuf> {
    /* `dirs` crate is already in the workspace via reqwest's transitive
     *  deps but not direct. Use std env to stay independent. */
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Walk from `cwd` upward to find `.claude/skills` dirs and discover
/// every `SKILL.md`. Stops at filesystem root. User-global skills
/// (`~/.claude/skills`) are appended last.
pub fn discover_skills(cwd: Option<&str>) -> Vec<Skill> {
    let mut out: Vec<Skill> = Vec::new();

    if let Some(cwd) = cwd {
        let start = Path::new(cwd);
        let mut current: Option<&Path> = Some(start);
        while let Some(p) = current {
            let candidate = p.join(".claude").join("skills");
            if candidate.is_dir() {
                read_dir_for_skills(&candidate, SkillScope::Project, &mut out);
            }
            current = p.parent();
        }
    }

    if let Some(user_dir) = user_skills_dir() {
        if user_dir.is_dir() {
            read_dir_for_skills(&user_dir, SkillScope::User, &mut out);
        }
    }

    /* Dedup by id — same skill name in project + user picks the
     *  project version (already first by walk order). */
    let mut seen = std::collections::HashSet::new();
    out.retain(|s| seen.insert(s.id.clone()));
    out
}

fn read_dir_for_skills(dir: &Path, scope: SkillScope, out: &mut Vec<Skill>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        if let Some(s) = load_skill(&skill_md, scope) {
            out.push(s);
        }
    }
}

fn load_skill(path: &Path, scope: SkillScope) -> Option<Skill> {
    let raw = std::fs::read_to_string(path).ok()?;
    let (front, body) = split_frontmatter(&raw);
    let parsed = parse_frontmatter(front);
    let dir_name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("unnamed")
        .to_string();
    let name = parsed
        .get("name")
        .cloned()
        .unwrap_or_else(|| dir_name.clone());
    let scope_prefix = match scope {
        SkillScope::User => "user",
        SkillScope::Project => "project",
    };
    let id = format!("{scope_prefix}:{name}");
    let allowed_tools = parsed
        .get("allowed-tools")
        .map(|v| parse_string_list(v))
        .unwrap_or_default();
    let extras: BTreeMap<String, String> = parsed
        .iter()
        .filter(|(k, _)| {
            !matches!(
                k.as_str(),
                "name"
                    | "description"
                    | "when_to_use"
                    | "when-to-use"
                    | "argument-hint"
                    | "argument_hint"
                    | "allowed-tools"
                    | "allowed_tools"
                    | "model"
            )
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    Some(Skill {
        id,
        name,
        description: parsed.get("description").cloned(),
        when_to_use: parsed
            .get("when_to_use")
            .or_else(|| parsed.get("when-to-use"))
            .cloned(),
        argument_hint: parsed
            .get("argument-hint")
            .or_else(|| parsed.get("argument_hint"))
            .cloned(),
        allowed_tools,
        model: parsed.get("model").cloned(),
        scope,
        path: path.to_string_lossy().into_owned(),
        body: body.to_string(),
        extras,
    })
}

// ---- Frontmatter parser --------------------------------------------------

fn split_frontmatter(raw: &str) -> (&str, &str) {
    /* YAML frontmatter starts with a `---` line and ends with another.
     *  Anything outside the fence is body. Missing fence → no
     *  frontmatter, the whole file is body. */
    let mut lines = raw.lines();
    let first = lines.next();
    if first != Some("---") {
        return ("", raw);
    }
    /* Find the closing `---`. We track byte offsets to slice safely
     *  back into `raw` without re-allocating. */
    let mut start = "---\n".len();
    if !raw.starts_with("---\n") {
        // Could be `---\r\n` — try the longer prefix.
        if raw.starts_with("---\r\n") {
            start = "---\r\n".len();
        } else {
            return ("", raw);
        }
    }
    let after = &raw[start..];
    if let Some(idx) = after.find("\n---\n") {
        let front = &after[..idx];
        let body_start = start + idx + "\n---\n".len();
        return (front, &raw[body_start..]);
    }
    if let Some(idx) = after.find("\r\n---\r\n") {
        let front = &after[..idx];
        let body_start = start + idx + "\r\n---\r\n".len();
        return (front, &raw[body_start..]);
    }
    // No closing fence — treat as no frontmatter.
    ("", raw)
}

fn parse_frontmatter(s: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in s.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some(colon) = trimmed.find(':') else { continue };
        let key = trimmed[..colon].trim().to_string();
        let value = trimmed[colon + 1..].trim();
        let value = strip_paired_quotes(value);
        out.insert(key, value);
    }
    out
}

fn strip_paired_quotes(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.len() >= 2 {
        let first = trimmed.chars().next();
        let last = trimmed.chars().last();
        if (first == Some('"') && last == Some('"'))
            || (first == Some('\'') && last == Some('\''))
        {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}

fn parse_string_list(s: &str) -> Vec<String> {
    let t = s.trim();
    if t.starts_with('[') && t.ends_with(']') {
        return t[1..t.len() - 1]
            .split(',')
            .map(|x| strip_paired_quotes(x.trim()))
            .filter(|x| !x.is_empty())
            .collect();
    }
    if t.is_empty() {
        return Vec::new();
    }
    vec![strip_paired_quotes(t)]
}

// ---- Rendering -----------------------------------------------------------

/// Maximum bytes of stdout we inject per `!` resolution. Above this
/// we truncate with a trailing `[…truncated]` marker. Keeps a runaway
/// `find /` from blowing the context window.
const SHELL_OUTPUT_CAP: usize = 100 * 1024;

/// Per-`!` command timeout. The shared `bash_run` enforces 10 min;
/// skill resolution should be faster — 30 s is generous for `git`,
/// `gh`, `kubectl`. If a user wants a longer wait, they can write the
/// command to background and have the skill body wait via `bg_wait_line`.
const SHELL_TIMEOUT_SECS: u64 = 30;

pub async fn render(
    skill: &Skill,
    arguments: &str,
    cwd: Option<&str>,
) -> RenderedSkill {
    let mut shell_results: Vec<ShellResult> = Vec::new();
    // Step 1: $ARGUMENTS substitution.
    let mut out = skill.body.replace("$ARGUMENTS", arguments);
    // Step 2: `` !`<cmd>` `` resolution. Cheap manual scan — we look
    // for the exact byte sequence `!\``, find the matching closing
    // backtick on the same line, run the inner command, and splice.
    out = expand_shell_blocks(&out, cwd.unwrap_or_else(|| skill_dir(&skill.path)), &mut shell_results)
        .await;
    RenderedSkill {
        skill: skill.clone(),
        rendered: out,
        shell_results,
    }
}

fn skill_dir(path: &str) -> &str {
    Path::new(path)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or(".")
}

async fn expand_shell_blocks(
    body: &str,
    cwd: &str,
    shell_results: &mut Vec<ShellResult>,
) -> String {
    let mut out = String::with_capacity(body.len());
    let mut i = 0usize;
    let bytes = body.as_bytes();
    while i < bytes.len() {
        // Look for the literal `!\`` sequence — must be preceded by
        // start-of-string or non-backslash so a literal `\\!` (an
        // intentionally-escaped exclamation) doesn't trigger.
        if i + 2 < bytes.len() && bytes[i] == b'!' && bytes[i + 1] == b'`' {
            let preceded_by_escape = i > 0 && bytes[i - 1] == b'\\';
            if !preceded_by_escape {
                // Find closing backtick on the same line.
                let inner_start = i + 2;
                let mut j = inner_start;
                let mut found = None;
                while j < bytes.len() {
                    let b = bytes[j];
                    if b == b'\n' || b == b'\r' {
                        break;
                    }
                    if b == b'`' {
                        found = Some(j);
                        break;
                    }
                    j += 1;
                }
                if let Some(close) = found {
                    let cmd = body[inner_start..close].to_string();
                    let resolved = run_shell_block(&cmd, cwd).await;
                    let mut piece = resolved.stdout.clone();
                    let truncated = piece.len() > SHELL_OUTPUT_CAP;
                    if truncated {
                        piece.truncate(SHELL_OUTPUT_CAP);
                        piece.push_str("\n[…truncated]");
                    }
                    if !resolved.ok {
                        // Surface failure inline so the agent sees what broke.
                        out.push_str(&format!(
                            "[shell-error: `{}` exited {}]\n{}",
                            cmd, resolved.code, piece
                        ));
                    } else {
                        out.push_str(&piece);
                    }
                    shell_results.push(ShellResult {
                        cmd,
                        ok: resolved.ok,
                        code: resolved.code,
                        stdout_truncated: truncated,
                    });
                    i = close + 1;
                    continue;
                }
            }
        }
        // No match — copy verbatim. UTF-8 safe because the matcher
        // operates on bytes that can only appear at start of a UTF-8
        // char (`!` and `` ` ``).
        let ch_end = utf8_char_end(bytes, i);
        out.push_str(&body[i..ch_end]);
        i = ch_end;
    }
    out
}

fn utf8_char_end(bytes: &[u8], i: usize) -> usize {
    /* Walk UTF-8 byte sequence length. ASCII fast path. */
    let b = bytes[i];
    if b < 0x80 {
        return i + 1;
    }
    let len = if b & 0b1110_0000 == 0b1100_0000 {
        2
    } else if b & 0b1111_0000 == 0b1110_0000 {
        3
    } else if b & 0b1111_1000 == 0b1111_0000 {
        4
    } else {
        1
    };
    (i + len).min(bytes.len())
}

struct ShellInvokeOutcome {
    stdout: String,
    code: i32,
    ok: bool,
}

async fn run_shell_block(cmd: &str, cwd: &str) -> ShellInvokeOutcome {
    /* Wrap `bash_run` with a tighter per-skill timeout. bash_run's own
     *  600s cap is fine as a backstop but we want a faster failure
     *  here so a skill that accidentally runs `sleep 1000` doesn't
     *  stall the chat send. */
    let fut = bash_run(cwd, cmd);
    match tokio::time::timeout(std::time::Duration::from_secs(SHELL_TIMEOUT_SECS), fut).await {
        Ok(Ok(r)) => ShellInvokeOutcome {
            stdout: if r.stdout.is_empty() {
                r.stderr
            } else if !r.stderr.is_empty() && !r.ok {
                /* On failure include stderr below stdout so the agent
                 *  sees error output. */
                format!("{}\n--- stderr ---\n{}", r.stdout, r.stderr)
            } else {
                r.stdout
            },
            code: r.code,
            ok: r.ok,
        },
        Ok(Err(e)) => ShellInvokeOutcome {
            stdout: format!("[bash_run error: {e}]"),
            code: -1,
            ok: false,
        },
        Err(_) => ShellInvokeOutcome {
            stdout: format!("[timeout after {SHELL_TIMEOUT_SECS}s]"),
            code: -1,
            ok: false,
        },
    }
}

// ---- Tauri commands ------------------------------------------------------

#[tauri::command]
pub fn skills_discover(cwd: Option<String>) -> Vec<Skill> {
    discover_skills(cwd.as_deref())
}

/// Frontend calls this once on app boot. Idempotent. Returns the
/// number of files written for diagnostic telemetry — usually 0 on
/// subsequent launches.
#[tauri::command]
pub fn skills_install_bundled_defaults() -> usize {
    install_bundled_defaults()
}

#[tauri::command]
pub async fn skills_render(
    id: String,
    arguments: String,
    cwd: Option<String>,
) -> Result<RenderedSkill, String> {
    let skills = discover_skills(cwd.as_deref());
    let Some(skill) = skills.into_iter().find(|s| s.id == id || s.name == id) else {
        return Err(format!("no such skill: {id}"));
    };
    Ok(render(&skill, &arguments, cwd.as_deref()).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_splits_at_fences() {
        let raw = "---\nname: foo\ndescription: bar\n---\nbody here\n";
        let (front, body) = split_frontmatter(raw);
        assert_eq!(front, "name: foo\ndescription: bar");
        assert_eq!(body, "body here\n");
    }

    #[test]
    fn frontmatter_missing_fence_returns_whole_body() {
        let raw = "no fence here\nbody body\n";
        let (front, body) = split_frontmatter(raw);
        assert_eq!(front, "");
        assert_eq!(body, raw);
    }

    #[test]
    fn parse_handles_quoted_and_unquoted() {
        let map = parse_frontmatter("name: foo\ndescription: \"some text\"\nargument-hint: '<pr>'");
        assert_eq!(map.get("name"), Some(&"foo".to_string()));
        assert_eq!(map.get("description"), Some(&"some text".to_string()));
        assert_eq!(map.get("argument-hint"), Some(&"<pr>".to_string()));
    }

    #[test]
    fn parse_string_list_handles_array_and_scalar() {
        assert_eq!(
            parse_string_list("[\"a\", \"b\", c]"),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert_eq!(parse_string_list("single"), vec!["single".to_string()]);
        assert_eq!(parse_string_list(""), Vec::<String>::new());
    }

    #[tokio::test]
    async fn shell_substitution_replaces_inline_backticks() {
        let mut results = Vec::new();
        let body = "before !`echo hello` after";
        let out = expand_shell_blocks(body, "/tmp", &mut results).await;
        assert!(out.contains("hello"), "got: {out:?}");
        assert!(out.starts_with("before "));
        assert!(out.trim_end().ends_with("after"));
        assert_eq!(results.len(), 1);
        assert!(results[0].ok);
    }

    #[tokio::test]
    async fn escaped_bang_preserved() {
        let mut results = Vec::new();
        let body = "literal \\!`not a command`";
        let out = expand_shell_blocks(body, "/tmp", &mut results).await;
        assert!(out.contains("\\!`not a command`"));
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn arguments_substitution() {
        let skill = Skill {
            id: "user:test".to_string(),
            name: "test".to_string(),
            description: None,
            when_to_use: None,
            argument_hint: None,
            allowed_tools: vec![],
            model: None,
            scope: SkillScope::User,
            path: "/tmp/test/SKILL.md".to_string(),
            body: "PR is $ARGUMENTS".to_string(),
            extras: BTreeMap::new(),
        };
        let r = render(&skill, "42", Some("/tmp")).await;
        assert_eq!(r.rendered, "PR is 42");
    }
}
