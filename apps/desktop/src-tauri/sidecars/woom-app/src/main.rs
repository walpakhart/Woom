//! woom-app — MCP sidecar that lets Claude / Cursor drive the
//! Woom UI itself: open detail panes, switch top-level views, add
//! editor instances, prompt the user to connect a missing source.
//!
//! How it works
//!   The sidecar's tools don't actually mutate the UI from here — they
//!   can't reach the running Woom process. Instead, each tool
//!   validates its arguments and returns a confirmation string. The
//!   Woom frontend's stream parser (`src/lib/stream/agentStream.ts`)
//!   sees the corresponding `mcp__app__*` tool_use event in Claude's
//!   stream-json output and performs the navigation directly via its
//!   already-reactive Svelte state. So the round-trip is:
//!
//!     Claude → tool_use {name: "mcp__app__open_jira_issue", input: …}
//!       → woom-app validates + replies "Opened DEVOPS-414."
//!       → frontend's stream handler also sees the tool_use, sets
//!         `inboxState.jira.focusKey = "DEVOPS-414"`, slide-over appears.
//!
//! The sidecar is intentionally thin — its main job is making the tools
//! callable so they show up in `--allowedTools` and the LLM has a
//! schema to fill in.

use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

mod terminal_bridge_client;
use terminal_bridge_client::BridgeClient;

/// Top-level view identifiers exposed to the agent. Named after the
/// platform (`github` / `jira` / `sentry`) rather than the generic noun
/// the UI used to show ("Repositories" / "Tasks" / "Issues") because:
///   1. We're going to grow more sources of the same shape — a future
///      GitLab tab would have its own "repositories" page, and the
///      agent would have no way to disambiguate.
///   2. The internal `View` enum on the frontend is `githubTab` /
///      `jiraTab` / `sentryTab` — so this also matches the UI rail
///      tooltips and the platform pills the user sees.
const VALID_VIEWS: &[&str] = &[
    "home",
    "github",
    "jira",
    "sentry",
    "claude",
    "cursor",
    "editor",
    "canvas",
    "terminal",
    "rules",
    "library",
    "connections",
    "settings",
];

const VALID_SOURCES: &[&str] = &[
    "github", "jira", "sentry", "claude", "cursor",
    "slack", "linear", "notion", "gitlab", "teams", "asana",
    "codex", "aider", "copilot",
];

const VALID_PR_TABS: &[&str] = &[
    "conversation", "commits", "files", "reviews", "checks",
];

const VALID_INSTANCE_KINDS: &[&str] = &[
    "github", "jira", "sentry", "claude", "cursor", "editor",
];

const VALID_REPO_SECTIONS: &[&str] = &[
    "code", "pulls", "issues", "actions", "releases",
];

const VALID_GH_FILTER_MODES: &[&str] = &[
    "involving", "authored", "review_requested", "assigned", "user", "all",
];

const VALID_SENTRY_STATUSES: &[&str] = &[
    "unresolved", "resolved", "ignored", "all",
];

const VALID_SENTRY_LEVELS: &[&str] = &[
    "all", "fatal", "error", "warning", "info", "debug",
];

#[derive(Clone)]
struct App {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenGithubPrParams {
    /// Repository owner (user or org), e.g. "Efficiently-Dev".
    owner: String,
    /// Repository name, e.g. "efficiently".
    repo: String,
    /// Pull request number.
    number: u64,
    /// Optional tab to focus inside the PR detail pane.
    /// One of: conversation | commits | files | reviews | checks.
    /// Defaults to "conversation".
    #[serde(default)]
    tab: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenGithubIssueParams {
    /// Repository owner.
    owner: String,
    /// Repository name.
    repo: String,
    /// Issue number.
    number: u64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenJiraIssueParams {
    /// Issue key (e.g. "DEVOPS-414"). Opens Woom's Jira slide-over
    /// pane on this issue.
    key: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenSentryIssueParams {
    /// Sentry issue id (numeric, e.g. "5728934712") OR short id
    /// (e.g. "BMS-API-J6"). Woom resolves short-ids when needed.
    id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenSentryEventParams {
    /// Sentry issue id (numeric or short id).
    issue_id: String,
    /// Specific event id to open. Omit (or pass "latest") to load the
    /// most recent occurrence — same as plain `open_sentry_issue`.
    /// Use a real event id to surface a particular occurrence (e.g. the
    /// one with the user's email or the failing release) instead of
    /// the latest. Pair with `mcp__sentry__list_events` to pick one.
    #[serde(default)]
    event_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SwitchViewParams {
    /// Which app / surface to switch to. One of:
    /// `home`, `github`, `jira`, `sentry`, `claude`, `cursor`, `editor`,
    /// `canvas`, `terminal`, `rules`, `library`, `connections`,
    /// `settings`. Each value maps to a full-screen solo (the rail's
    /// icons map 1:1 to these names).
    view: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenConnectModalParams {
    /// Source/agent id whose connect modal to open. One of:
    /// github, jira, sentry, claude, cursor, slack, linear, notion,
    /// gitlab, teams, asana, codex, aider, copilot. The modal renders
    /// install / token instructions appropriate for the source.
    source: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AddAppInstanceParams {
    /// Which app to spawn. One of: github, jira, sentry, claude, cursor,
    /// editor, canvas, terminal. Singleton apps (github/jira/sentry/
    /// claude/cursor) just switch the rail; only editor/canvas/terminal
    /// actually create a fresh instance. Accepts `kind` or `type`.
    #[serde(alias = "type")]
    kind: String,
    /// Only meaningful when `kind = "editor"`. Absolute folder path to
    /// open in the new editor instance. Omit for an empty editor.
    ///
    /// Aliases: `path`, `folder`, `directory`, `cwd`, `repo`, `repoPath`.
    #[serde(
        default,
        alias = "path",
        alias = "folder",
        alias = "directory",
        alias = "cwd",
        alias = "repo",
        alias = "repoPath"
    )]
    repo_path: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenRepoParams {
    /// Repository owner (user or org).
    owner: String,
    /// Repository name.
    repo: String,
    /// Which section of the repo to land on. One of: code (file
    /// browser), pulls (PR list), issues (issue list), actions (workflow
    /// runs), releases. Defaults to "pulls".
    #[serde(default)]
    section: Option<String>,
    /// Optional repo-relative path to open inside `section=code`. When
    /// set, Woom drills into the code browser to that file (or
    /// folder) — e.g. `src/lib/auth.ts`. Ignored for non-code sections.
    /// Use this when the user says "open <file> in <repo>" instead of
    /// just "open <repo>".
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenJiraTabParams {
    /// Project key to filter by (e.g. `DEVOPS`). Null/omitted = all
    /// projects. Resolve human names via `mcp__jira__list_projects`.
    #[serde(default)]
    project_key: Option<String>,
    /// Free-text JQL fragment appended to the search query (e.g.
    /// `"login flow"`). The tab's existing UI builds the full JQL —
    /// just pass the keyword.
    #[serde(default)]
    search: Option<String>,
    /// Status name like "In Review", "Done", "Blocked". Use the human
    /// label, the tab handles canonicalisation.
    #[serde(default)]
    status_name: Option<String>,
    /// Numeric Jira board ids to scope to. Multi-select — multiple
    /// boards OR-merge their projects. Resolve via the JQL search +
    /// board metadata; usually omit unless the user explicitly
    /// mentioned a board.
    #[serde(default)]
    board_ids: Option<Vec<u64>>,
    /// Numeric sprint ids (or `"backlog"`). Resolve via
    /// `mcp__jira__list_sprints`. Only meaningful with exactly one
    /// board selected.
    #[serde(default)]
    sprint_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenSentryTabParams {
    /// Sentry project slugs to filter by (e.g. `["catalog-api",
    /// "checkout-web"]`). Empty/omitted = all projects. Resolve via
    /// `mcp__sentry__list_projects`.
    #[serde(default)]
    projects: Option<Vec<String>>,
    /// Free-text search appended to Sentry's search query (e.g.
    /// `"login flow"`). Combined with the structured filters below.
    #[serde(default)]
    search: Option<String>,
    /// Issue status filter. One of: unresolved, resolved, ignored, all.
    /// Defaults to keeping the user's current selection.
    #[serde(default)]
    status: Option<String>,
    /// Severity level filter. One of: all, fatal, error, warning, info,
    /// debug.
    #[serde(default)]
    level: Option<String>,
    /// Environment slug (e.g. `production`, `staging`). Null = all envs.
    #[serde(default)]
    environment: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetGithubInstanceParams {
    /// Art-name of the github app instance (e.g. "Petra"). Either
    /// `instance_name` or `instance_id` must be provided.
    #[serde(default)]
    instance_name: Option<String>,
    /// UUID of the github instance.
    #[serde(default)]
    instance_id: Option<String>,
    /// `owner/name` to scope the instance to (e.g. `Efficiently-Dev/efficiently`).
    /// Pass empty string `""` to clear the repo filter (= all repos).
    #[serde(default)]
    repo: Option<String>,
    /// Filter mode. One of: involving, authored, review_requested,
    /// assigned, user, all. Mirrors the dropdown in the instance header.
    #[serde(default)]
    mode: Option<String>,
    /// Free-text search applied on top of `mode` + `repo` filters.
    #[serde(default)]
    search: Option<String>,
    /// GitHub login when `mode = "user"`. Ignored otherwise.
    #[serde(default)]
    custom_user: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetJiraInstanceParams {
    /// Art-name of the jira app instance (e.g. "Mona-Lisa").
    /// Either `instance_name` or `instance_id` must be provided.
    #[serde(default)]
    instance_name: Option<String>,
    /// UUID of the jira instance.
    #[serde(default)]
    instance_id: Option<String>,
    /// Project key (e.g. `DEVOPS`). Empty string clears the filter.
    #[serde(default)]
    project_key: Option<String>,
    /// Status name ("In Review", "Done"). Empty clears.
    #[serde(default)]
    status_name: Option<String>,
    /// Free-text search.
    #[serde(default)]
    search: Option<String>,
    /// Numeric board ids — see OpenJiraTabParams.
    #[serde(default)]
    board_ids: Option<Vec<u64>>,
    /// Sprint ids — see OpenJiraTabParams.
    #[serde(default)]
    sprint_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetSentryInstanceParams {
    /// Art-name of the sentry app instance. Either `instance_name`
    /// or `instance_id` must be provided.
    #[serde(default)]
    instance_name: Option<String>,
    /// UUID of the sentry instance.
    #[serde(default)]
    instance_id: Option<String>,
    /// Sentry project slugs.
    #[serde(default)]
    projects: Option<Vec<String>>,
    /// Free-text search.
    #[serde(default)]
    search: Option<String>,
    /// Status filter. unresolved/resolved/ignored/all.
    #[serde(default)]
    status: Option<String>,
    /// Severity level. all/fatal/error/warning/info/debug.
    #[serde(default)]
    level: Option<String>,
    /// Environment slug.
    #[serde(default)]
    environment: Option<String>,
}

/// Field names we recognise as `repo_path`. The first element is the
/// canonical form. Used both as serde aliases on the struct AND by the
/// recursive extractor when the LLM nests args under `args` /
/// `arguments` / `params` / `input`.
const REPO_PATH_KEYS: &[&str] = &[
    "repo_path",
    "repoPath",
    "path",
    "folder",
    "directory",
    "dir",
    "cwd",
    "repo",
    "repository_path",
    "folderPath",
    "dirPath",
    "fullPath",
    "absolutePath",
    "target_path",
    "target",
];
const INSTANCE_NAME_KEYS: &[&str] = &[
    "instance_name",
    "instanceName",
    "name",
    "editor_name",
    "agent_name",
    "label",
];
const INSTANCE_ID_KEYS: &[&str] = &[
    "instance_id",
    "instanceId",
    "id",
    "editor_id",
    "agent_id",
    "uuid",
];

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetEditorRepoPathParams {
    /// Art-name of the editor instance (e.g. "Sagrada-Familia"). Either
    /// `instance_name` or `instance_id` must be provided. Names are
    /// matched case-insensitively. Accepts the alias `name`.
    ///
    /// Schema-as-`Option<String>` is intentional — see the comment on
    /// `repo_path` below. Runtime type is still `Option<Value>` so
    /// `coerce_to_string` can salvage non-string shapes.
    #[serde(default, alias = "name", alias = "instanceName")]
    #[schemars(with = "Option<String>")]
    instance_name: Option<serde_json::Value>,
    /// UUID of the editor instance. Either `instance_name` or
    /// `instance_id` must be provided. Accepts the alias `id`.
    #[serde(default, alias = "id", alias = "instanceId")]
    #[schemars(with = "Option<String>")]
    instance_id: Option<serde_json::Value>,
    /// Absolute folder path to open in the editor. If the editor has
    /// linked agent sessions, their cwd is auto-updated to match.
    ///
    /// Two-faced typing — runtime `Option<Value>`, advertised
    /// `Option<String>` via `schemars(with = …)`:
    ///
    /// - Runtime is `Option<Value>` because cursor-agent has been
    ///   observed shipping this field as an array, a wrapped object,
    ///   or an empty string. `coerce_to_string` salvages the inner
    ///   path; recursive search through `extras` is the last fallback.
    ///
    /// - The advertised JSON Schema MUST declare a real `type`
    ///   (`["string", "null"]`). Without that key, cursor-agent's
    ///   tool-binder strips the field entirely from the LLM's call
    ///   before the request reaches us — `repo_path=None` arrives
    ///   regardless of what the model wrote. Claude is more lenient
    ///   here, which is why the same prompt works on Claude but
    ///   fails on Cursor without this attribute. The model sees
    ///   `string` in the catalog, emits a string, and our runtime
    ///   `Value` decodes the string just fine.
    #[serde(
        default,
        alias = "path",
        alias = "folder",
        alias = "directory",
        alias = "dir",
        alias = "cwd",
        alias = "repo",
        alias = "repoPath",
        alias = "folderPath",
        alias = "dirPath",
        alias = "fullPath",
        alias = "absolutePath"
    )]
    #[schemars(with = "Option<String>")]
    repo_path: Option<serde_json::Value>,
    /// Catch-all for any other keys the LLM happened to produce —
    /// most importantly wrappers like `{"args": {"repo_path": …}}`,
    /// which our handler will recursively search.
    #[serde(flatten)]
    extras: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetAgentCwdParams {
    /// Use `target = "self"` to point at the calling session (most
    /// common when the user says "switch myself"). Otherwise pass
    /// `instance_name` or `instance_id` of the target agent instance.
    #[serde(default)]
    target: Option<String>,
    /// Art-name of the agent instance. Optional — only used when
    /// `target` is omitted or != "self". Accepts the alias `name`.
    /// Same `schemars(with = …)` rationale as `SetEditorRepoPathParams`.
    #[serde(default, alias = "name", alias = "instanceName")]
    #[schemars(with = "Option<String>")]
    instance_name: Option<serde_json::Value>,
    /// UUID of the agent instance. Accepts the alias `id`.
    #[serde(default, alias = "id", alias = "instanceId")]
    #[schemars(with = "Option<String>")]
    instance_id: Option<serde_json::Value>,
    /// Absolute folder path to use as cwd. The change takes effect on
    /// the agent session's NEXT turn (the current turn keeps the old
    /// cwd it spawned with).
    ///
    /// Aliases: `path`, `folder`, `directory`, `cwd`, `repo`, `repoPath`,
    /// `folderPath`, `dirPath`, `fullPath`, `absolutePath`. Same Value
    /// trick AND `schemars(with = …)` rationale as
    /// `SetEditorRepoPathParams::repo_path` — without the schema
    /// override, cursor-agent silently drops this field.
    #[serde(
        default,
        alias = "path",
        alias = "folder",
        alias = "directory",
        alias = "dir",
        alias = "cwd",
        alias = "repo",
        alias = "repoPath",
        alias = "folderPath",
        alias = "dirPath",
        alias = "fullPath",
        alias = "absolutePath"
    )]
    #[schemars(with = "Option<String>")]
    repo_path: Option<serde_json::Value>,
    /// Catch-all — see `SetEditorRepoPathParams::extras`.
    #[serde(flatten)]
    extras: std::collections::BTreeMap<String, serde_json::Value>,
}

/// Coerce a Value into a non-empty trimmed string when possible. cursor-
/// agent has shipped this field as:
///   - String("/Users/me/repo")            — happy path
///   - Array(["/Users/me/repo"])           — single-element wrap
///   - Object({"path": "/Users/me/repo"}) — over-eager nesting
///   - String("")                          — empty placeholder
/// Any of these yields a valid path string; everything else returns None.
fn coerce_to_string(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => {
            let t = s.trim();
            if t.is_empty() { None } else { Some(t.to_string()) }
        }
        serde_json::Value::Array(arr) => arr.iter().find_map(coerce_to_string),
        serde_json::Value::Object(obj) => {
            // Common nested shapes: {"path": "..."}, {"value": "..."},
            // {"text": "..."}. Prefer keys that look path-ish.
            for k in REPO_PATH_KEYS.iter().chain(["value", "text", "string"].iter()) {
                if let Some(inner) = obj.get(*k) {
                    if let Some(s) = coerce_to_string(inner) {
                        return Some(s);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Recursively search a Value for the first key in `keys` whose value
/// coerces to a non-empty string. Walks through wrapper objects like
/// `{"args": …}` / `{"arguments": …}` / `{"params": …}` / `{"input": …}`
/// up to a small depth so a malformed `{"args":{"args":{"repo_path":…}}}`
/// still resolves. Stops at a fixed depth to avoid infinite recursion
/// on cyclic structures (which serde_json::Value can't actually make,
/// but we cap anyway).
fn find_field_recursive(
    value: &serde_json::Value,
    keys: &[&str],
    depth: u8,
) -> Option<String> {
    if depth == 0 {
        return None;
    }
    let serde_json::Value::Object(map) = value else {
        return None;
    };
    for key in keys {
        if let Some(found) = map.get(*key) {
            if let Some(s) = coerce_to_string(found) {
                return Some(s);
            }
        }
    }
    // Walk known wrapper keys. cursor-agent / claude have both been seen
    // wrapping arguments: `{"args": …}` is the most common, but other
    // OpenAI-flavoured CLIs use `arguments`/`params`/`input`.
    for wrapper in ["args", "arguments", "params", "parameters", "input", "data", "payload"] {
        if let Some(inner) = map.get(wrapper) {
            if let Some(s) = find_field_recursive(inner, keys, depth - 1) {
                return Some(s);
            }
        }
    }
    None
}

/// Find a `repo_path` value across all the places we accept it. Order
/// of precedence:
///   1. The typed `repo_path` field (already covers all serde aliases).
///   2. Recursive search through the typed extras map (catches
///      `{"args": {"repo_path": …}}` and friends).
fn extract_repo_path(
    typed: &Option<serde_json::Value>,
    extras: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Option<String> {
    if let Some(v) = typed.as_ref().and_then(coerce_to_string) {
        return Some(v);
    }
    let extras_value = serde_json::Value::Object(extras.clone().into_iter().collect());
    find_field_recursive(&extras_value, REPO_PATH_KEYS, 4)
}

fn extract_typed_or_recursive(
    typed: &Option<serde_json::Value>,
    extras: &std::collections::BTreeMap<String, serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    if let Some(v) = typed.as_ref().and_then(coerce_to_string) {
        return Some(v);
    }
    let extras_value = serde_json::Value::Object(extras.clone().into_iter().collect());
    find_field_recursive(&extras_value, keys, 4)
}

// ---------- Canvas (whiteboard) param shapes ----------
//
// Architecture mirrors the existing pattern: this sidecar validates the
// arguments and returns a confirmation string. The Woom frontend's
// stream parser sees the same `mcp__app__canvas_*` tool_use event in
// Claude's stream-json output and performs the actual mutation against
// the canvas store.
//
// READ access is NOT via a tool call. The frontend injects a compact
// state summary of the linked canvas into the system prompt at the
// start of every turn, so the agent already knows the shape inventory
// + their bboxes. That keeps the round-trip free of IPC.
//
// Agent-supplied ids: every shape and edge insert lets the agent pass
// the id it wants to use (`shape_id` / `edge_id`). Without it, the
// frontend mints one and includes it in the confirmation string the
// agent reads — but providing one up front lets the agent reference
// the shape in a subsequent edge call without a round-trip.

const VALID_SHAPE_KINDS: &[&str] = &[
    "rect", "ellipse", "arrow-shape", "line", "text", "sticky",
    "mermaid", "dot", "code", "image", "freehand",
    "frame", "group",
    "jira-card", "github-pr-card", "github-issue-card",
    "sentry-event-card", "file-card", "chat-message-card",
];

const VALID_EDGE_ANCHORS: &[&str] = &[
    "tl", "tc", "tr", "ml", "mc", "mr", "bl", "bc", "br",
];

const VALID_EDGE_KINDS: &[&str] = &["arrow", "line", "dashed"];
const VALID_EDGE_ROUTINGS: &[&str] = &["straight", "orthogonal", "curved"];

const VALID_LAYOUT_ALGORITHMS: &[&str] = &["grid", "row", "column", "dagre"];

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasAddShapeParams {
    /// Optional client-supplied id for the new shape. Useful when you'll
    /// reference it in a subsequent `canvas_add_edge` call. Must be a
    /// uuid-like string. Omit to let Woom mint one — the new id
    /// will be in the confirmation text so you can grab it.
    #[serde(default)]
    shape_id: Option<String>,
    /// Shape kind. One of: rect, ellipse, arrow-shape, line, text,
    /// sticky, mermaid, dot, code, image, freehand, frame, group, and
    /// the live-card kinds (jira-card, github-pr-card, github-issue-card,
    /// sentry-event-card, file-card, chat-message-card). For live cards
    /// you must also pass the lookup keys in `props` (e.g. ticketKey for
    /// jira-card; owner/repo/number for github-pr-card).
    kind: String,
    /// Top-left x in canvas pixels.
    x: f64,
    /// Top-left y in canvas pixels.
    y: f64,
    /// Width in canvas pixels (>0).
    w: f64,
    /// Height in canvas pixels (>0).
    h: f64,
    /// Optional kind-specific properties (text source, mermaid source,
    /// stroke / fill colors, sticky markdown, etc). Forge merges this
    /// with the kind's defaults. See docs/CANVAS.md §5 for the schema.
    #[serde(default)]
    #[allow(dead_code)]
    props: Option<serde_json::Value>,
    /// Optional accessibility label. Shown on hover; doesn't affect
    /// rendering otherwise.
    #[serde(default)]
    #[allow(dead_code)]
    label: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasAddShapesParams {
    /// Batch of shape specs to insert atomically (single undo entry).
    /// Each entry has the same fields as `canvas_add_shape`. Use this
    /// when drawing a multi-shape diagram (e.g. 5 boxes + their layout)
    /// so the user can ⌘Z back to the pre-insert state in one step.
    shapes: Vec<CanvasAddShapeParams>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasUpdateShapeParams {
    /// Shape id to patch.
    shape_id: String,
    /// Optional new top-left x.
    #[serde(default)] x: Option<f64>,
    #[serde(default)] y: Option<f64>,
    /// Optional new width / height. Both > 0 if provided.
    #[serde(default)] w: Option<f64>,
    #[serde(default)] h: Option<f64>,
    /// Optional new rotation in radians.
    #[serde(default)] rot: Option<f64>,
    /// Optional kind-specific props patch. Merges with existing props
    /// (does NOT replace — pass only the fields you want to change).
    #[serde(default)]
    props: Option<serde_json::Value>,
    /// Optional new accessibility label.
    #[serde(default)]
    label: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasDeleteShapeParams {
    /// Single shape id to delete. Use either `shape_id` OR
    /// `shape_ids` — at least one is required.
    #[serde(default)]
    shape_id: Option<String>,
    /// Multiple shape ids to delete in one history entry. Edges
    /// touching deleted shapes are removed alongside (cascade).
    #[serde(default)]
    shape_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasAddEdgeParams {
    /// Optional client-supplied id for the new edge.
    #[serde(default, alias = "id", alias = "edgeId")]
    edge_id: Option<String>,
    /// Source shape id.
    ///
    /// Required, but typed as `Option<String>` so we can return a
    /// helpful hint when an LLM forgets the field. Aliases:
    /// `from`, `source`, `from_id`, `fromId`, `fromShapeId`,
    /// `fromNode`, `fromBlock`, `start`, `start_id`, `startId`,
    /// `src`, `sourceId`.
    #[serde(
        default,
        alias = "from",
        alias = "source",
        alias = "from_id",
        alias = "fromId",
        alias = "fromShapeId",
        alias = "fromNode",
        alias = "fromBlock",
        alias = "start",
        alias = "start_id",
        alias = "startId",
        alias = "src",
        alias = "sourceId"
    )]
    from_shape_id: Option<String>,
    /// Source anchor — one of: tl, tc, tr, ml, mc, mr, bl, bc, br.
    /// Defaults to `mr` (right-middle) for left-to-right flow.
    /// Aliases: `fromAnchor`, `source_anchor`, `sourceAnchor`,
    /// `start_anchor`, `startAnchor`, `srcAnchor`.
    #[serde(
        default,
        alias = "fromAnchor",
        alias = "source_anchor",
        alias = "sourceAnchor",
        alias = "start_anchor",
        alias = "startAnchor",
        alias = "srcAnchor"
    )]
    from_anchor: Option<String>,
    /// Target shape id. Aliases:
    /// `to`, `target`, `to_id`, `toId`, `toShapeId`, `toNode`,
    /// `toBlock`, `end`, `end_id`, `endId`, `dest`, `dst`, `targetId`.
    #[serde(
        default,
        alias = "to",
        alias = "target",
        alias = "to_id",
        alias = "toId",
        alias = "toShapeId",
        alias = "toNode",
        alias = "toBlock",
        alias = "end",
        alias = "end_id",
        alias = "endId",
        alias = "dest",
        alias = "dst",
        alias = "targetId"
    )]
    to_shape_id: Option<String>,
    /// Target anchor — same options as `from_anchor`. Defaults to `ml`.
    /// Aliases: `toAnchor`, `target_anchor`, `targetAnchor`,
    /// `end_anchor`, `endAnchor`, `destAnchor`.
    #[serde(
        default,
        alias = "toAnchor",
        alias = "target_anchor",
        alias = "targetAnchor",
        alias = "end_anchor",
        alias = "endAnchor",
        alias = "destAnchor"
    )]
    to_anchor: Option<String>,
    /// Visual style. One of: arrow (default — directed), line, dashed.
    /// Accepts the aliases `style`, `edge_kind`, `edgeKind`.
    #[serde(default, alias = "style", alias = "edge_kind", alias = "edgeKind")]
    kind: Option<String>,
    /// Routing algorithm. One of: straight, orthogonal (default —
    /// Manhattan elbow), curved (cubic bezier). Aliases: `route`,
    /// `path`, `pathing`.
    #[serde(default, alias = "route", alias = "path", alias = "pathing")]
    routing: Option<String>,
    /// Optional mid-line label. Accepts the aliases `text`, `caption`,
    /// `title`.
    #[serde(default, alias = "text", alias = "caption", alias = "title")]
    #[allow(dead_code)] /* read by the frontend dispatcher from raw JSON; the sidecar's confirmation doesn't surface it. */
    label: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasAddEdgesParams {
    /// Batch of edge specs to insert atomically (single undo entry).
    /// Each entry has the same fields as `canvas_add_edge` — including
    /// the alias-friendly field names (`from`/`to` etc.). Use this
    /// when wiring up a multi-edge diagram so it lands as one ⌘Z step
    /// instead of N. Aliases for this top-level field: `connections`,
    /// `links`, `arrows`.
    #[serde(alias = "connections", alias = "links", alias = "arrows")]
    edges: Vec<CanvasAddEdgeParams>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasDeleteEdgeParams {
    /// Edge id to delete (or use `edge_ids` for bulk).
    #[serde(default)]
    edge_id: Option<String>,
    #[serde(default)]
    edge_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasArrangeParams {
    /// Layout algorithm. One of: `grid`, `row`, `column`, `dagre`.
    /// `dagre` is best for connected DAGs (it uses the existing edges
    /// to compute layers); `row` / `column` produce a linear sequence;
    /// `grid` packs into a square-ish array.
    algorithm: String,
    /// Optional list of shape ids to arrange. If omitted, layouts the
    /// entire canvas's root-level shapes.
    #[serde(default)]
    shape_ids: Option<Vec<String>>,
    /// Optional rankdir for `dagre` only. One of: TB, LR (default), BT,
    /// RL. Ignored for the other algorithms.
    #[serde(default)]
    rankdir: Option<String>,
    /// Optional gap between shapes in canvas px (used by grid/row/
    /// column). Defaults to 24.
    #[serde(default)]
    #[allow(dead_code)]
    gap: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasFocusParams {
    /// Shape id to scroll/zoom into the visible viewport. The user
    /// sees a smooth animation toward the shape — useful right after
    /// you add new shapes off-screen.
    shape_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasSetZParams {
    /// Shape id to reorder.
    shape_id: String,
    /// Z-order action. `to-front` floats above everything; `to-back`
    /// sinks below everything; `forward` swaps just above the next
    /// higher shape; `backward` just below the next lower one.
    mode: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasDuplicateParams {
    /// Shape ids to clone. New shapes get fresh uuids and are offset
    /// by `(dx, dy)` canvas px from the originals so they don't sit
    /// flush on top. Default offset (12, 12) matches Figma's ⌘D.
    shape_ids: Vec<String>,
    #[serde(default)] #[allow(dead_code)] dx: Option<f64>,
    #[serde(default)] #[allow(dead_code)] dy: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasFindParams {
    /// Substring to search for. Case-insensitive. Matches shape
    /// labels, text content, mermaid / DOT / code source, sticky
    /// markdown, live-card lookup keys (ticketKey, relPath, shortId,
    /// snapshot.title, snapshot.summary). Returns matched shape ids
    /// in creation order.
    query: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasGroupParams {
    /// Shape ids to wrap in a fresh frame/group container. The
    /// container is sized to their AABB plus padding and each child's
    /// `parentId` is set to the new container — so dragging the
    /// container moves all of them together.
    shape_ids: Vec<String>,
    /// Container kind: `frame` (default — visible labeled rectangle)
    /// or `group` (logical container with no visual border).
    #[serde(default)]
    #[allow(dead_code)]
    kind: Option<String>,
    /// Optional label for the frame's title bar. Ignored for `group`.
    #[serde(default)]
    #[allow(dead_code)]
    title: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasUngroupParams {
    /// Container shape id (a `frame` or `group`). Children get their
    /// `parentId` cleared and the container is removed.
    shape_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasLockParams {
    /// Shape ids to (un)lock.
    shape_ids: Vec<String>,
    /// `true` = lock (further patches and edits ignored), `false` =
    /// unlock.
    locked: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasAlignParams {
    /// Shape ids to align. Need at least 2.
    shape_ids: Vec<String>,
    /// Alignment axis. One of: `left`, `center-x`, `right`, `top`,
    /// `center-y`, `bottom`. The anchor value is derived from the
    /// AABB of the selection (e.g. `left` snaps every shape's left
    /// edge to the leftmost current left edge).
    axis: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasDistributeParams {
    /// Shape ids to distribute. Need at least 3 — first and last
    /// keep their position; the middle ones are spaced equally.
    shape_ids: Vec<String>,
    /// Distribution axis. `horizontal` (equalize gaps between
    /// columns) or `vertical` (between rows).
    axis: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasSetViewportParams {
    /// Viewport top-left x in canvas pixels. Required.
    x: f64,
    /// Viewport top-left y in canvas pixels. Required.
    y: f64,
    /// Zoom multiplier (canvas-px → CSS-px). 1.0 = identity. Clamped
    /// to 0.1..4.0 to match the manual zoom range. Optional —
    /// defaults to current zoom.
    #[serde(default)]
    zoom: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CanvasUploadImageParams {
    /// Base64-encoded image bytes (PNG or JPEG; no `data:...;base64,`
    /// prefix). The frontend decodes via dataURL, computes intrinsic
    /// size, and inserts an `image` shape clamped to a max-dim cap so
    /// a giant image doesn't dominate the canvas.
    base64: String,
    /// Image MIME type. One of: `image/png`, `image/jpeg`,
    /// `image/gif`, `image/webp`. Used for the dataURL prefix.
    /// Defaults to `image/png`.
    #[serde(default)]
    #[allow(dead_code)]
    mime_type: Option<String>,
    /// Top-left x where the image lands. Defaults to current viewport
    /// center if omitted.
    #[serde(default)]
    #[allow(dead_code)]
    x: Option<f64>,
    /// Top-left y. Defaults to current viewport center if omitted.
    #[serde(default)]
    #[allow(dead_code)]
    y: Option<f64>,
    /// Optional shape id for the new image (lets you reference it in
    /// follow-up calls).
    #[serde(default)]
    #[allow(dead_code)]
    shape_id: Option<String>,
    /// Optional alt text.
    #[serde(default)]
    #[allow(dead_code)]
    alt: Option<String>,
}

fn validate_one_of(value: &str, choices: &[&str], label: &str) -> Result<(), ErrorData> {
    if choices.contains(&value) {
        Ok(())
    } else {
        Err(ErrorData::invalid_params(
            format!(
                "invalid {label} `{value}`. expected one of: {}",
                choices.join(", ")
            ),
            None,
        ))
    }
}

/// Pull a non-empty `instance_name` first, fall back to `instance_id`,
/// or fail with the same error every instance-mutating tool wants.
/// Centralised because every `set_*_instance` tool needs the exact same
/// dispatch and we don't want to copy 6 lines of `.trim().filter().or()`
/// six times.
fn require_instance_label<'a>(
    name: Option<&'a str>,
    id: Option<&'a str>,
) -> Result<&'a str, ErrorData> {
    let by_name = name.map(str::trim).filter(|s| !s.is_empty());
    let by_id = id.map(str::trim).filter(|s| !s.is_empty());
    by_name.or(by_id).ok_or_else(|| {
        ErrorData::invalid_params(
            "either `instance_name` or `instance_id` must be provided",
            None,
        )
    })
}

/// Tiny accumulator that builds a "k=v, k=v" trace string for the human-
/// readable confirmation a tool returns. The agent already sees the
/// structured `Parameters(…)` it sent, but the user sees this string in
/// the trace pill — so we want it readable, comma-separated, and skip
/// keys the caller didn't touch. Keeping it as a helper because the
/// three instance-mutator tools (github/jira/sentry) follow the same
/// "patch only the keys you want" semantics and would otherwise each
/// reimplement the same Vec push + join.
struct FilterBits(Vec<String>);

fn changed_filter_bits() -> FilterBits {
    FilterBits(Vec::new())
}

impl FilterBits {
    fn push_kv(&mut self, key: &str, value: &str) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.0.push(format!("{}=<cleared>", key));
        } else {
            self.0.push(format!("{}=\"{}\"", key, trimmed));
        }
    }

    fn summary(&self, empty: &str) -> String {
        if self.0.is_empty() {
            empty.to_string()
        } else {
            self.0.join(", ")
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProposeBashParams {
    /// The exact shell command you want to run (single line, will be run via `sh -c`).
    command: String,
    /// Short free-form explanation for the user of why this command is needed.
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProposeSwitchCwdParams {
    /// Absolute local path to switch the Claude session's working directory to.
    path: String,
    /// A short free-form note for the user explaining why you want to switch.
    #[serde(default)]
    reason: Option<String>,
}

#[tool_router]
impl App {
    fn new() -> Self {
        Self { tool_router: Self::tool_router() }
    }

    #[tool(
        description = "Open a GitHub pull request in Woom's PR detail pane (the slide-over Woom normally opens when the user clicks a PR card). Optional `tab` selects which sub-tab is focused on open: `conversation` (default — comments + reviews), `commits`, `files` (diff browser), `reviews`, `checks` (CI). Use this when the user says \"open PR #X\" or asks to look at a specific PR — it gives them the same view they'd get clicking through the inbox themselves."
    )]
    async fn open_github_pr(
        &self,
        Parameters(OpenGithubPrParams { owner, repo, number, tab }): Parameters<OpenGithubPrParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if let Some(t) = tab.as_deref() {
            validate_one_of(t, VALID_PR_TABS, "tab")?;
        }
        let tab_label = tab.as_deref().unwrap_or("conversation");
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening {}/{}#{} ({} tab) in Woom's PR detail pane.",
            owner, repo, number, tab_label
        ))]))
    }

    #[tool(
        description = "Open a GitHub issue in Woom's detail pane — same as open_github_pr but for plain issues (not PRs). Use when the user references an issue by repo + number."
    )]
    async fn open_github_issue(
        &self,
        Parameters(OpenGithubIssueParams { owner, repo, number }): Parameters<OpenGithubIssueParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening {}/{}#{} (issue) in Woom's detail pane.",
            owner, repo, number
        ))]))
    }

    #[tool(
        description = "Open a Jira issue in Woom's slide-over pane. Same as the user clicking the ticket from the Jira solo — shows description, comments, transitions, worklog. Use when the user says \"show DEVOPS-414\" or wants to look at a specific ticket."
    )]
    async fn open_jira_issue(
        &self,
        Parameters(OpenJiraIssueParams { key }): Parameters<OpenJiraIssueParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let trimmed = key.trim();
        if trimmed.is_empty() {
            return Err(ErrorData::invalid_params("issue key is empty", None));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening Jira {} in Woom's detail pane.",
            trimmed
        ))]))
    }

    #[tool(
        description = "Open a Sentry issue in Woom's slide-over detail pane. Accepts either the numeric id or the short id (e.g. `BMS-API-J6` — Woom resolves it). Use when the user says \"show BMS-API-J6\" or wants to drill into a specific Sentry issue."
    )]
    async fn open_sentry_issue(
        &self,
        Parameters(OpenSentryIssueParams { id }): Parameters<OpenSentryIssueParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            return Err(ErrorData::invalid_params("issue id is empty", None));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening Sentry issue {} in Woom's detail pane.",
            trimmed
        ))]))
    }

    #[tool(
        description = "Open a SPECIFIC event of a Sentry issue in Woom's detail pane (instead of just the latest). Use this when you've called mcp__sentry__list_events and want to surface one particular occurrence — e.g. \"the one in production after release 2.4.1\" or \"the one where user X hit it\". Pass `issue_id` (numeric or short id) and `event_id` (the real event id from list_events). Omit event_id to fall back to latest, equivalent to open_sentry_issue."
    )]
    async fn open_sentry_event(
        &self,
        Parameters(OpenSentryEventParams { issue_id, event_id }): Parameters<OpenSentryEventParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let issue = issue_id.trim();
        if issue.is_empty() {
            return Err(ErrorData::invalid_params("issue_id is empty", None));
        }
        let event = event_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("latest");
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening Sentry issue {} on event {} in Woom's detail pane.",
            issue, event
        ))]))
    }

    #[tool(
        description = "Switch Woom's active solo (top-level app). One of: `home`, `github`, `jira`, `sentry`, `claude`, `cursor`, `editor`, `canvas`, `terminal`, `rules`, `library`, `connections`, `settings`. Each value maps 1:1 to a rail icon. Use when the user wants to navigate (\"open github\", \"go to jira\", \"show me sentry issues\"). For SCOPED navigation (specific repo / project / sprint / sentry filter), prefer `open_github_repo` / `open_jira_tab` / `open_sentry_tab` instead — they switch the solo AND apply filters in one call."
    )]
    async fn switch_view(
        &self,
        Parameters(SwitchViewParams { view }): Parameters<SwitchViewParams>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_one_of(&view, VALID_VIEWS, "view")?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Switching Woom view → {}.",
            view
        ))]))
    }

    #[tool(
        description = "Open the connect / status modal for a source or agent. Use this when the user mentions an integration that isn't connected yet (\"do I have Slack hooked up?\", \"connect Sentry\") — surface the modal so they can finish setup. Source ids: github, jira, sentry, claude, cursor, slack, linear, notion, gitlab, teams, asana, codex, aider, copilot."
    )]
    async fn open_connect_modal(
        &self,
        Parameters(OpenConnectModalParams { source }): Parameters<OpenConnectModalParams>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_one_of(&source, VALID_SOURCES, "source")?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening connect modal for `{}`.",
            source
        ))]))
    }

    #[tool(
        description = "Spawn a new instance of an app. Kinds: `github` (PR/issue inbox), `jira` (Jira inbox), `sentry` (Sentry issues inbox), `claude` (Claude chat), `cursor` (Cursor chat), `editor` (file browser + editor), `canvas` (whiteboard), `terminal` (PTY shell). For `editor`, optionally pass `repo_path` to open a folder immediately. Singleton apps (github/jira/sentry/claude/cursor) ignore the spawn and just switch the rail to them — only `editor`/`canvas`/`terminal` actually support multiple instances. Use whenever the user asks to \"open another Editor for /Users/me/Repos/foo\" / \"give me a second terminal\"."
    )]
    async fn add_app_instance(
        &self,
        Parameters(AddAppInstanceParams { kind, repo_path }): Parameters<AddAppInstanceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_one_of(&kind, VALID_INSTANCE_KINDS, "kind")?;
        let path = repo_path.as_deref().map(str::trim).filter(|s| !s.is_empty());
        match (kind.as_str(), path) {
            ("editor", Some(p)) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Spawning a new Editor instance pointed at `{}`.",
                p
            ))])),
            ("editor", None) => Ok(CallToolResult::success(vec![Content::text(
                "Spawning a new empty Editor instance.".to_string(),
            )])),
            (k, _) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Spawning a new `{}` instance.",
                k
            ))])),
        }
    }

    #[tool(
        description = "Open a specific GitHub repository inside Woom's GitHub tab. Switches the top-level view to `github`, picks the repo from the list, and lands on `section` (default: `pulls`). Sections: `code` (file browser + README), `pulls` (PR list), `issues` (issue list), `actions` (workflow runs), `releases`. Pass `path` together with `section=code` to drill into a specific file or folder (e.g. `src/lib/auth.ts`) — the file viewer opens with the contents preloaded. Named `open_github_repo` rather than `open_repo` because we'll grow into other VCS sources (GitLab, Bitbucket, etc.) where \"repository\" lookups need their own resolver. Use whenever the user wants to drill into a GitHub repo (\"show me the actions for efficiently\", \"open src/lib/auth.ts in efficiently\", \"open the releases tab on forge\")."
    )]
    async fn open_github_repo(
        &self,
        Parameters(OpenRepoParams { owner, repo, section, path }): Parameters<OpenRepoParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if owner.trim().is_empty() || repo.trim().is_empty() {
            return Err(ErrorData::invalid_params("owner and repo are required", None));
        }
        if let Some(s) = section.as_deref() {
            validate_one_of(s, VALID_REPO_SECTIONS, "section")?;
        }
        let s = section.as_deref().unwrap_or("pulls");
        let trimmed_path = path.as_deref().map(str::trim).filter(|p| !p.is_empty());
        if trimmed_path.is_some() && s != "code" {
            return Err(ErrorData::invalid_params(
                "`path` is only valid when `section` is `code`",
                None,
            ));
        }
        let suffix = match trimmed_path {
            Some(p) => format!(" → file `{}`", p),
            None => String::new(),
        };
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening repository {}/{} → {} section{}.",
            owner.trim(),
            repo.trim(),
            s,
            suffix
        ))]))
    }

    #[tool(
        description = "Open the Jira top-level tab with optional filters applied — same controls as the tab's UI dropdowns. `project_key` scopes to one project, `status_name` filters by workflow state (\"In Review\", \"Done\"), `search` is a free-text fragment. `board_ids` and `sprint_ids` use Jira's numeric ids — resolve them via `mcp__jira__list_sprints` first if the user mentions a sprint by name. Omit a parameter to leave that filter as the user last set it. Use this instead of `switch_view view=jira` whenever the user asks for a SPECIFIC slice (\"show me my open Jira tickets in DEVOPS\", \"sprint 160 tickets\", \"in-review tickets\")."
    )]
    async fn open_jira_tab(
        &self,
        Parameters(OpenJiraTabParams { project_key, search, status_name, board_ids, sprint_ids }): Parameters<OpenJiraTabParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut bits: Vec<String> = Vec::new();
        if let Some(p) = project_key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            bits.push(format!("project={}", p));
        }
        if let Some(s) = status_name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            bits.push(format!("status=\"{}\"", s));
        }
        if let Some(q) = search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            bits.push(format!("search=\"{}\"", q));
        }
        if let Some(b) = board_ids.as_ref().filter(|v| !v.is_empty()) {
            bits.push(format!(
                "boards=[{}]",
                b.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")
            ));
        }
        if let Some(s) = sprint_ids.as_ref().filter(|v| !v.is_empty()) {
            bits.push(format!("sprints=[{}]", s.join(",")));
        }
        let summary = if bits.is_empty() {
            "no filters (showing whatever the tab last had)".to_string()
        } else {
            bits.join(", ")
        };
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening Jira tab with {}.",
            summary
        ))]))
    }

    #[tool(
        description = "Open the Sentry top-level tab with optional filters — same controls as the tab's filter bar. `projects` scopes to one or more Sentry project slugs, `status` is unresolved/resolved/ignored/all, `level` is fatal/error/warning/info/debug/all, `environment` is the env slug (e.g. `production`), `search` is free-text appended to Sentry's query. Omit a parameter to leave it untouched. Use whenever the user asks for a SPECIFIC slice of Sentry issues (\"production crashes\", \"unresolved errors in checkout-web\") instead of just \"open Sentry\"."
    )]
    async fn open_sentry_tab(
        &self,
        Parameters(OpenSentryTabParams { projects, search, status, level, environment }): Parameters<OpenSentryTabParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if let Some(s) = status.as_deref() {
            validate_one_of(s, VALID_SENTRY_STATUSES, "status")?;
        }
        if let Some(l) = level.as_deref() {
            validate_one_of(l, VALID_SENTRY_LEVELS, "level")?;
        }
        let mut bits: Vec<String> = Vec::new();
        if let Some(p) = projects.as_ref().filter(|v| !v.is_empty()) {
            bits.push(format!("projects=[{}]", p.join(",")));
        }
        if let Some(q) = search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            bits.push(format!("search=\"{}\"", q));
        }
        if let Some(s) = status.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            bits.push(format!("status={}", s));
        }
        if let Some(l) = level.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            bits.push(format!("level={}", l));
        }
        if let Some(e) = environment.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            bits.push(format!("env={}", e));
        }
        let summary = if bits.is_empty() {
            "no filters (showing whatever the tab last had)".to_string()
        } else {
            bits.join(", ")
        };
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening Sentry tab with {}.",
            summary
        ))]))
    }

    #[tool(
        description = "Apply filters to the GitHub app instance (kind=`github`). Identify by `instance_name` (art-name like \"Petra\") or `instance_id`. `repo` is `owner/name` (or empty string `\"\"` to clear → all repos), `mode` is involving/authored/review_requested/assigned/user/all, `search` is free-text, `custom_user` is a GitHub login (only used when `mode=user`). Pass only the keys you want to change; omitted keys keep their current value. Use this when a GitHub instance already exists — \"show authored PRs in efficiently in github\", \"filter Petra to only review-requested\"."
    )]
    async fn set_github_instance(
        &self,
        Parameters(SetGithubInstanceParams { instance_name, instance_id, repo, mode, search, custom_user }): Parameters<SetGithubInstanceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let label = require_instance_label(instance_name.as_deref(), instance_id.as_deref())?;
        if let Some(m) = mode.as_deref() {
            validate_one_of(m, VALID_GH_FILTER_MODES, "mode")?;
        }
        let mut bits = changed_filter_bits();
        if let Some(r) = repo {
            bits.push_kv("repo", &r);
        }
        if let Some(m) = mode {
            bits.push_kv("mode", &m);
        }
        if let Some(s) = search {
            bits.push_kv("search", &s);
        }
        if let Some(u) = custom_user {
            bits.push_kv("custom_user", &u);
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Updating github instance `{}`: {}.",
            label,
            bits.summary("no filter changes")
        ))]))
    }

    #[tool(
        description = "Apply filters to the Jira app instance (kind=`jira`). Identify by `instance_name` or `instance_id`. `project_key` (empty string clears), `status_name`, `search`, `board_ids`, `sprint_ids` — same semantics as `open_jira_tab`. Pass only what you want to change. Use when a Jira instance already exists and the user asks for a different scope (\"narrow Mona-Lisa to DEVOPS\", \"show in-review only in jira\")."
    )]
    async fn set_jira_instance(
        &self,
        Parameters(SetJiraInstanceParams { instance_name, instance_id, project_key, status_name, search, board_ids, sprint_ids }): Parameters<SetJiraInstanceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let label = require_instance_label(instance_name.as_deref(), instance_id.as_deref())?;
        let mut bits = changed_filter_bits();
        if let Some(p) = project_key {
            bits.push_kv("project", &p);
        }
        if let Some(s) = status_name {
            bits.push_kv("status", &s);
        }
        if let Some(q) = search {
            bits.push_kv("search", &q);
        }
        if let Some(b) = board_ids {
            bits.push_kv(
                "boards",
                &format!("[{}]", b.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")),
            );
        }
        if let Some(s) = sprint_ids {
            bits.push_kv("sprints", &format!("[{}]", s.join(",")));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Updating jira instance `{}`: {}.",
            label,
            bits.summary("no filter changes")
        ))]))
    }

    #[tool(
        description = "Apply filters to the Sentry app instance (kind=`sentry`). Identify by `instance_name` or `instance_id`. `projects`, `search`, `status`, `level`, `environment` — same semantics as `open_sentry_tab`. Pass only what you want to change. Use to retarget the existing Sentry instance (\"only show production fatals in sentry\")."
    )]
    async fn set_sentry_instance(
        &self,
        Parameters(SetSentryInstanceParams { instance_name, instance_id, projects, search, status, level, environment }): Parameters<SetSentryInstanceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let label = require_instance_label(instance_name.as_deref(), instance_id.as_deref())?;
        if let Some(s) = status.as_deref() {
            validate_one_of(s, VALID_SENTRY_STATUSES, "status")?;
        }
        if let Some(l) = level.as_deref() {
            validate_one_of(l, VALID_SENTRY_LEVELS, "level")?;
        }
        let mut bits = changed_filter_bits();
        if let Some(p) = projects {
            bits.push_kv("projects", &format!("[{}]", p.join(",")));
        }
        if let Some(q) = search {
            bits.push_kv("search", &q);
        }
        if let Some(s) = status {
            bits.push_kv("status", &s);
        }
        if let Some(l) = level {
            bits.push_kv("level", &l);
        }
        if let Some(e) = environment {
            bits.push_kv("env", &e);
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Updating sentry instance `{}`: {}.",
            label,
            bits.summary("no filter changes")
        ))]))
    }

    #[tool(
        description = "Change the open folder of an EXISTING editor instance. Use this when the user says \"switch the editor to /path\" — do NOT use add_app_instance, which spawns a NEW editor. Identify the editor by `instance_name` (the art-name like \"Sagrada-Familia\" shown in the rail popover) or `instance_id`. If the editor has linked agent sessions, their cwd is auto-updated to match (no separate set_agent_cwd call needed)."
    )]
    async fn set_editor_repo_path(
        &self,
        Parameters(SetEditorRepoPathParams { instance_name, instance_id, repo_path, extras }): Parameters<SetEditorRepoPathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Try the typed slot first, then walk extras for wrapper objects.
        let path = extract_repo_path(&repo_path, &extras);
        let name = extract_typed_or_recursive(&instance_name, &extras, INSTANCE_NAME_KEYS);
        let id = extract_typed_or_recursive(&instance_id, &extras, INSTANCE_ID_KEYS);

        let received_summary = format!(
            "instance_name={:?}, instance_id={:?}, repo_path={:?}, extras_keys={:?}",
            instance_name, instance_id, repo_path,
            extras.keys().collect::<Vec<_>>()
        );

        let path = path.ok_or_else(|| {
            eprintln!(
                "[woom-app] set_editor_repo_path: could not resolve repo_path. {}",
                received_summary
            );
            ErrorData::invalid_params(
                format!("`repo_path` is required. Accepted top-level keys: `repo_path`, `path`, `folder`, `directory`, `dir`, `cwd`, `repo`, `repoPath`, `folderPath`, `dirPath`, `fullPath`, `absolutePath`. The value can be a string OR a single-element array. The whole arguments object can also be wrapped under `args` / `arguments` / `params` / `input`. Got: {}. Pass an absolute folder path, e.g. `/Users/me/Repos/foo`.", received_summary),
                None,
            )
        })?;

        if name.is_none() && id.is_none() {
            eprintln!(
                "[woom-app] set_editor_repo_path: missing instance_name/instance_id. {}",
                received_summary
            );
            return Err(ErrorData::invalid_params(
                format!("either `instance_name` (alias `name`, `instanceName`, `editor_name`, `label`) or `instance_id` (alias `id`, `instanceId`, `editor_id`, `uuid`) must be provided. Got: {}. The art-name is the one shown in the rail popover — e.g. \"Sagrada-Familia\".", received_summary),
                None,
            ));
        }
        let label = name.as_deref().or(id.as_deref()).unwrap_or("editor");
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Setting editor `{}` repo path → `{}`. Linked agent sessions (if any) update too.",
            label, path
        ))]))
    }

    #[tool(
        description = "Change an agent session's cwd (working directory). Use when the user says \"switch yourself to /path\" / \"point Claude at /repo\" / \"have the cursor agent work on X\". For yourself, pass `target=\"self\"` — the OS process running this turn keeps its old spawn-time cwd, BUT you can keep working in the new repo within the same turn by addressing files with absolute paths (Read/Write/Edit) and prefixing shell commands with `cd <new_path> && ...`. The next turn spawns fresh with the new cwd as default, so the absolute-path workaround is a one-turn thing. For another agent instance, pass `instance_name` (e.g. \"Mona-Lisa\") or `instance_id`. Do NOT use this to create a new agent — use add_app_instance for that."
    )]
    async fn set_agent_cwd(
        &self,
        Parameters(SetAgentCwdParams { target, instance_name, instance_id, repo_path, extras }): Parameters<SetAgentCwdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = extract_repo_path(&repo_path, &extras);
        let name = extract_typed_or_recursive(&instance_name, &extras, INSTANCE_NAME_KEYS);
        let id = extract_typed_or_recursive(&instance_id, &extras, INSTANCE_ID_KEYS);

        let received_summary = format!(
            "target={:?}, instance_name={:?}, instance_id={:?}, repo_path={:?}, extras_keys={:?}",
            target, instance_name, instance_id, repo_path,
            extras.keys().collect::<Vec<_>>()
        );

        let path = path.ok_or_else(|| {
            eprintln!(
                "[woom-app] set_agent_cwd: could not resolve repo_path. {}",
                received_summary
            );
            ErrorData::invalid_params(
                format!("`repo_path` is required. Accepted top-level keys: `repo_path`, `path`, `folder`, `directory`, `dir`, `cwd`, `repo`, `repoPath`, `folderPath`, `dirPath`, `fullPath`, `absolutePath`. The value can be a string OR a single-element array. The whole arguments object can also be wrapped under `args` / `arguments` / `params` / `input`. Got: {}. Pass an absolute folder path, e.g. `/Users/me/Repos/foo`.", received_summary),
                None,
            )
        })?;

        let is_self = target.as_deref().map(str::trim).map(|s| s.eq_ignore_ascii_case("self")).unwrap_or(false);
        if !is_self {
            if name.is_none() && id.is_none() {
                return Err(ErrorData::invalid_params(
                    format!("for non-self target, either `instance_name` (alias `name`, `instanceName`) or `instance_id` (alias `id`, `instanceId`) must be provided. To target the calling session itself, pass `target=\"self\"` instead. Got: {}", received_summary),
                    None,
                ));
            }
            let label = name.as_deref().or(id.as_deref()).unwrap_or("agent");
            // The other agent's session uuid + recap are rotated by
            // Woom's frontend interceptor; from THIS agent's
            // perspective the message is just "ack, change recorded."
            // No absolute-path note here — that other agent will pick
            // up the new cwd on its OWN next turn.
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "Setting agent `{}` cwd → `{}` (takes effect on its next turn — the user's next message in that chat will spawn the agent in the new directory with a recap injected).",
                label, path
            ))]));
        }
        // Self-switch within an in-flight turn. Spell out the workaround
        // explicitly so the agent doesn't burn the rest of the turn
        // resolving relative paths against a stale cwd. Process working
        // directory is fork-time-only on POSIX; we can't change it on a
        // running child without killing it, and killing mid-turn would
        // throw away the current agent state. Absolute paths sidestep
        // it cleanly: Read/Write/Edit accept them natively, and Bash
        // takes a `cd <path> &&` prefix.
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Setting MY (self) cwd → `{path}`.\n\n\
             IMPORTANT — this turn vs. next turn:\n\
             • The OS process running this turn was spawned with the OLD cwd. Its working directory cannot be changed mid-process, so Read/Write/Edit/Bash WITHOUT absolute paths still resolve under the old root.\n\
             • To act in the new repo within THIS SAME turn, use absolute paths for every subsequent tool call:\n\
                 - Read / Write / Edit: pass `{path}/relative/file.ext` instead of `relative/file.ext`.\n\
                 - Bash: prefix every command with `cd {path} && …` (or use absolute paths in the command).\n\
             • Your NEXT turn will be spawned with `{path}` as the default cwd automatically. A recap of recent exchanges is injected into that turn's system prompt for continuity, so you can drop the `cd` prefix and absolute paths from then on.\n\
             • This isn't an error — it's the cost of switching projects without killing the in-flight agent loop."
        ))]))
    }

    #[tool(
        description = "Re-list the app instances open in Woom. The runtime injects this state into your system prompt at the start of every turn, so you usually already know it. Call this only if you suspect the preamble is stale (e.g. you just spawned a new instance and want to confirm its name/id), or if the user references something that wasn't in the preamble."
    )]
    async fn list_instances(&self) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::text(
            "Woom injects the current app-instance map into your system prompt at the start of every turn. Re-read it for the latest state. (This tool is a placeholder — the frontend interceptor doesn't need to mutate anything for it; the actual data lives in the system prompt preamble.)".to_string(),
        )]))
    }

    // ---------- Canvas (whiteboard) ----------

    #[tool(
        description = "Add a single shape to the canvas the current session is linked to. Coordinates are in canvas pixels (logical, DPI-independent). For live cards (jira-card, github-pr-card, etc.), pass the lookup keys in `props` — e.g. `{\"ticketKey\": \"PROJ-123\"}` for jira-card. Pass an explicit `shape_id` (any uuid-like string) when you'll reference the shape in a later `canvas_add_edge` call so you don't have to round-trip. The current canvas state is in the system prompt preamble (id, dimensions, shape inventory) — read from there to know where to place new shapes."
    )]
    async fn canvas_add_shape(
        &self,
        Parameters(p): Parameters<CanvasAddShapeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_one_of(&p.kind, VALID_SHAPE_KINDS, "kind")?;
        if !(p.w.is_finite() && p.w > 0.0) || !(p.h.is_finite() && p.h > 0.0) {
            return Err(ErrorData::invalid_params("w and h must be > 0", None));
        }
        if !p.x.is_finite() || !p.y.is_finite() {
            return Err(ErrorData::invalid_params("x and y must be finite numbers", None));
        }
        let id_label = p.shape_id.as_deref().unwrap_or("(auto)");
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Adding `{}` shape (id `{}`) at ({:.0},{:.0}) {:.0}x{:.0} on the linked canvas.",
            p.kind, id_label, p.x, p.y, p.w, p.h
        ))]))
    }

    #[tool(
        description = "Add MANY shapes to the linked canvas in one atomic op (single ⌘Z entry). Use this when laying out a multi-shape diagram — it lands as one history step instead of N. Each entry has the same fields as `canvas_add_shape`. After the call, you can use `canvas_arrange` to position them via dagre/grid/row/column without computing positions yourself."
    )]
    async fn canvas_add_shapes(
        &self,
        Parameters(p): Parameters<CanvasAddShapesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if p.shapes.is_empty() {
            return Err(ErrorData::invalid_params("`shapes` is empty", None));
        }
        for s in &p.shapes {
            validate_one_of(&s.kind, VALID_SHAPE_KINDS, "kind")?;
            if !(s.w.is_finite() && s.w > 0.0) || !(s.h.is_finite() && s.h > 0.0) {
                return Err(ErrorData::invalid_params(
                    format!("shape '{}' has invalid w/h (must be > 0)", s.kind),
                    None,
                ));
            }
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Adding {} shape(s) to the linked canvas (atomic).",
            p.shapes.len()
        ))]))
    }

    #[tool(
        description = "Patch a shape on the linked canvas. Pass only the fields you want to change — others are preserved. To MOVE: pass new x/y. To RESIZE: pass new w/h. To CHANGE TEXT (text/sticky): pass `props.text` or `props.markdown`. To CHANGE MERMAID: pass `props.source`. The patch is one undo step; multi-field patches collapse to one ⌘Z."
    )]
    async fn canvas_update_shape(
        &self,
        Parameters(p): Parameters<CanvasUpdateShapeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if p.shape_id.trim().is_empty() {
            return Err(ErrorData::invalid_params("`shape_id` is empty", None));
        }
        if let (Some(w), _) = (p.w, ()) { if !w.is_finite() || w <= 0.0 {
            return Err(ErrorData::invalid_params("w must be > 0", None));
        }}
        if let (Some(h), _) = (p.h, ()) { if !h.is_finite() || h <= 0.0 {
            return Err(ErrorData::invalid_params("h must be > 0", None));
        }}
        let mut bits = Vec::new();
        if let Some(x) = p.x { bits.push(format!("x={:.0}", x)); }
        if let Some(y) = p.y { bits.push(format!("y={:.0}", y)); }
        if let Some(w) = p.w { bits.push(format!("w={:.0}", w)); }
        if let Some(h) = p.h { bits.push(format!("h={:.0}", h)); }
        if let Some(r) = p.rot { bits.push(format!("rot={:.2}", r)); }
        if p.props.is_some() { bits.push("props=…".to_string()); }
        if p.label.is_some() { bits.push("label=…".to_string()); }
        let summary = if bits.is_empty() { "(no changes)".to_string() } else { bits.join(", ") };
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Patching shape `{}`: {}.",
            p.shape_id, summary
        ))]))
    }

    #[tool(
        description = "Delete a shape (or many) from the linked canvas. Edges anchored to the deleted shape(s) are removed in the same history entry — undo restores both at once. Provide either `shape_id` (single) or `shape_ids` (array)."
    )]
    async fn canvas_delete_shape(
        &self,
        Parameters(p): Parameters<CanvasDeleteShapeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let single = p.shape_id.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let many = p.shape_ids.as_ref().filter(|v| !v.is_empty());
        if single.is_none() && many.is_none() {
            return Err(ErrorData::invalid_params(
                "either `shape_id` or `shape_ids` is required",
                None,
            ));
        }
        let n = match (single, many) {
            (Some(_), Some(arr)) => arr.len() + 1,
            (Some(_), None) => 1,
            (None, Some(arr)) => arr.len(),
            _ => 0,
        };
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Deleting {} shape(s) (and their connecting edges) on the linked canvas.",
            n
        ))]))
    }

    #[tool(
        description = "Connect two shapes with an edge on the linked canvas. Anchors are the canonical 9-point set per shape (`tl`,`tc`,`tr`,`ml`,`mc`,`mr`,`bl`,`bc`,`br`); for a left-to-right flowchart the defaults `from_anchor=mr` + `to_anchor=ml` give clean straight handoffs. `routing` controls the path: `orthogonal` (default — Manhattan elbow, best for boxy diagrams), `straight` (no detour), `curved` (cubic bezier, organic feel). `kind` controls visuals: `arrow` (default — directed), `line`, `dashed`."
    )]
    async fn canvas_add_edge(
        &self,
        Parameters(p): Parameters<CanvasAddEdgeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        /* Echo back what we received when something is missing. LLMs
           that loop on this tool produce a lot of stderr noise, so
           keep messages tight but unambiguous. */
        let received_summary = format!(
            "from_shape_id={:?}, to_shape_id={:?}, from_anchor={:?}, to_anchor={:?}, kind={:?}, routing={:?}, edge_id={:?}, label={:?}",
            p.from_shape_id, p.to_shape_id, p.from_anchor, p.to_anchor,
            p.kind, p.routing, p.edge_id, p.label
        );
        let from = p
            .from_shape_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                eprintln!(
                    "[woom-app] canvas_add_edge: missing from_shape_id. Received: {}",
                    received_summary
                );
                ErrorData::invalid_params(
                    format!("`from_shape_id` is required. Aliases accepted: `from`, `source`, `from_id`, `fromId`, `fromShapeId`, `fromNode`, `start`, `src`. Got: {}. Use a shape id from `canvas_add_shape` confirmations or the canvas-state preamble.", received_summary),
                    None,
                )
            })?;
        let to = p
            .to_shape_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                eprintln!(
                    "[woom-app] canvas_add_edge: missing to_shape_id. Received: {}",
                    received_summary
                );
                ErrorData::invalid_params(
                    format!("`to_shape_id` is required. Aliases accepted: `to`, `target`, `to_id`, `toId`, `toShapeId`, `toNode`, `end`, `dest`, `dst`. Got: {}.", received_summary),
                    None,
                )
            })?;
        if let Some(a) = p.from_anchor.as_deref() { validate_one_of(a, VALID_EDGE_ANCHORS, "from_anchor")?; }
        if let Some(a) = p.to_anchor.as_deref()   { validate_one_of(a, VALID_EDGE_ANCHORS, "to_anchor")?; }
        if let Some(k) = p.kind.as_deref()        { validate_one_of(k, VALID_EDGE_KINDS, "kind")?; }
        if let Some(r) = p.routing.as_deref()     { validate_one_of(r, VALID_EDGE_ROUTINGS, "routing")?; }
        let id_label = p.edge_id.as_deref().unwrap_or("(auto)");
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Connecting `{}` → `{}` (edge id `{}`, {} routing).",
            from,
            to,
            id_label,
            p.routing.as_deref().unwrap_or("orthogonal")
        ))]))
    }

    #[tool(
        description = "Add MANY edges to the linked canvas in one atomic op (single ⌘Z entry). Use this when wiring up a flowchart — it lands as one history step instead of N. Each entry has the same fields as `canvas_add_edge` (including the alias-friendly names `from`/`to`). Top-level field is `edges`; aliases `connections`, `links`, `arrows` are also accepted."
    )]
    async fn canvas_add_edges(
        &self,
        Parameters(p): Parameters<CanvasAddEdgesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if p.edges.is_empty() {
            return Err(ErrorData::invalid_params("`edges` array is empty — pass at least one edge spec.", None));
        }
        for (i, e) in p.edges.iter().enumerate() {
            let from = e.from_shape_id.as_deref().map(str::trim).filter(|s| !s.is_empty());
            let to = e.to_shape_id.as_deref().map(str::trim).filter(|s| !s.is_empty());
            if from.is_none() || to.is_none() {
                let summary = format!(
                    "from_shape_id={:?}, to_shape_id={:?}",
                    e.from_shape_id, e.to_shape_id
                );
                eprintln!(
                    "[woom-app] canvas_add_edges: edge[{}] missing shape ids. Received: {}",
                    i, summary
                );
                return Err(ErrorData::invalid_params(
                    format!("edge[{}] missing shape ids — both `from_shape_id` and `to_shape_id` are required (aliases: from/to/source/target/...). Got: {}.", i, summary),
                    None,
                ));
            }
            if let Some(a) = e.from_anchor.as_deref() { validate_one_of(a, VALID_EDGE_ANCHORS, "from_anchor")?; }
            if let Some(a) = e.to_anchor.as_deref()   { validate_one_of(a, VALID_EDGE_ANCHORS, "to_anchor")?; }
            if let Some(k) = e.kind.as_deref()        { validate_one_of(k, VALID_EDGE_KINDS, "kind")?; }
            if let Some(r) = e.routing.as_deref()     { validate_one_of(r, VALID_EDGE_ROUTINGS, "routing")?; }
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Connecting {} edge(s) on the linked canvas (atomic).",
            p.edges.len()
        ))]))
    }

    #[tool(
        description = "Delete one or more edges from the linked canvas. Provide `edge_id` (single) or `edge_ids` (bulk)."
    )]
    async fn canvas_delete_edge(
        &self,
        Parameters(p): Parameters<CanvasDeleteEdgeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let single = p.edge_id.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let many = p.edge_ids.as_ref().filter(|v| !v.is_empty());
        if single.is_none() && many.is_none() {
            return Err(ErrorData::invalid_params(
                "either `edge_id` or `edge_ids` is required",
                None,
            ));
        }
        let n = match (single, many) {
            (Some(_), Some(arr)) => arr.len() + 1,
            (Some(_), None) => 1,
            (None, Some(arr)) => arr.len(),
            _ => 0,
        };
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Deleting {} edge(s) on the linked canvas.",
            n
        ))]))
    }

    #[tool(
        description = "Auto-position shapes on the linked canvas via a built-in layout algorithm. Use this AFTER `canvas_add_shapes` so you don't have to compute positions yourself. Algorithms: `dagre` (Sugiyama-style layered DAG, uses existing edges — best for flowcharts; pass `rankdir`=LR/TB), `grid` (square-ish pack), `row` (horizontal sequence), `column` (vertical). With `shape_ids` empty / omitted, layouts every root-level shape on the canvas. The new positions land as one undo entry."
    )]
    async fn canvas_arrange(
        &self,
        Parameters(p): Parameters<CanvasArrangeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_one_of(&p.algorithm, VALID_LAYOUT_ALGORITHMS, "algorithm")?;
        if let Some(rd) = p.rankdir.as_deref() {
            validate_one_of(rd, &["TB", "LR", "BT", "RL"], "rankdir")?;
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Arranging {} shape(s) with `{}` layout on the linked canvas.",
            p.shape_ids.as_ref().map(|v| v.len().to_string()).unwrap_or_else(|| "all".to_string()),
            p.algorithm
        ))]))
    }

    #[tool(
        description = "Animate the canvas viewport to bring a shape into view. Smooth pan/zoom toward the shape so the user sees what you just added or modified. Useful right after adding shapes off-screen — gives the user a visual cue that a new piece of the diagram exists."
    )]
    async fn canvas_focus(
        &self,
        Parameters(CanvasFocusParams { shape_id }): Parameters<CanvasFocusParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if shape_id.trim().is_empty() {
            return Err(ErrorData::invalid_params("`shape_id` is required", None));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Focusing canvas viewport on shape `{}`.",
            shape_id
        ))]))
    }

    #[tool(
        description = "Reorder a shape in the canvas's z-stack. `mode=to-front` floats it above everything; `to-back` sinks it below; `forward` / `backward` swap with the next-adjacent neighbour. Use when shapes overlap and you need a specific one on top — e.g. a callout sticky over a screenshot."
    )]
    async fn canvas_set_z(
        &self,
        Parameters(CanvasSetZParams { shape_id, mode }): Parameters<CanvasSetZParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if shape_id.trim().is_empty() {
            return Err(ErrorData::invalid_params("`shape_id` is required", None));
        }
        validate_one_of(&mode, &["to-front", "to-back", "forward", "backward"], "mode")?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Setting z-order of `{}` → {}.",
            shape_id, mode
        ))]))
    }

    #[tool(
        description = "Clone shape(s) on the linked canvas. Each clone gets a fresh id and is offset by `(dx, dy)` canvas px (default 12, 12). Useful when you want to vary a layout — duplicate a node, then patch its label / position. The clones are auto-selected after the call so a follow-up `canvas_arrange` works on them. Returns one undo entry."
    )]
    async fn canvas_duplicate(
        &self,
        Parameters(p): Parameters<CanvasDuplicateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if p.shape_ids.is_empty() {
            return Err(ErrorData::invalid_params("`shape_ids` is empty", None));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Duplicating {} shape(s) on the linked canvas.",
            p.shape_ids.len()
        ))]))
    }

    #[tool(
        description = "Substring-search the linked canvas. Case-insensitive — matches shape labels, text content, mermaid/DOT/code source, sticky markdown, and live-card lookup fields (ticketKey, relPath, shortId, snapshot title/summary). Returns matched shape ids you can then update / focus / connect. The system-prompt preamble already contains an inventory; use this tool when you need to find by content rather than browse by id."
    )]
    async fn canvas_find(
        &self,
        Parameters(CanvasFindParams { query }): Parameters<CanvasFindParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if query.trim().is_empty() {
            return Err(ErrorData::invalid_params("`query` is empty", None));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Searching the linked canvas for `{}`. Result ids will be visible in your next system-prompt preamble — re-issue your inventory check or assume a match.",
            query.trim()
        ))]))
    }

    #[tool(
        description = "Wrap shapes in a frame / group container so they move together as a unit. Each child's `parentId` is set to the new container — drag the container, all children follow. Use this when you've drawn a multi-shape sub-diagram and want to treat it as one piece (\"group these auth flow boxes so I can move them together\"). The container's bbox auto-sizes to the children + padding."
    )]
    async fn canvas_group(
        &self,
        Parameters(p): Parameters<CanvasGroupParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if p.shape_ids.is_empty() {
            return Err(ErrorData::invalid_params("`shape_ids` is empty", None));
        }
        if let Some(k) = p.kind.as_deref() {
            validate_one_of(k, &["frame", "group"], "kind")?;
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Grouping {} shape(s) into a {}.",
            p.shape_ids.len(),
            p.kind.as_deref().unwrap_or("frame")
        ))]))
    }

    #[tool(
        description = "Inverse of canvas_group — unwraps a frame/group, freeing its children to root. Children keep their absolute positions; the container is removed. The freed children become the new selection so a follow-up `canvas_arrange` works on them."
    )]
    async fn canvas_ungroup(
        &self,
        Parameters(CanvasUngroupParams { shape_id }): Parameters<CanvasUngroupParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if shape_id.trim().is_empty() {
            return Err(ErrorData::invalid_params("`shape_id` is empty", None));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Ungrouping container `{}` — children become root-level.",
            shape_id
        ))]))
    }

    #[tool(
        description = "Lock or unlock shapes. Locked shapes ignore further patches (move / resize / props) — useful when the user has \"frozen\" reference cards and you should rearrange the rest. Pass `locked=false` to unlock."
    )]
    async fn canvas_lock(
        &self,
        Parameters(p): Parameters<CanvasLockParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if p.shape_ids.is_empty() {
            return Err(ErrorData::invalid_params("`shape_ids` is empty", None));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "{} {} shape(s).",
            if p.locked { "Locking" } else { "Unlocking" },
            p.shape_ids.len()
        ))]))
    }

    #[tool(
        description = "Align selected shapes on an axis. The anchor (snap-to value) is derived from the selection's AABB — e.g. `left` aligns every shape's left edge to the leftmost current left edge; `center-x` centers them on the horizontal mid-line of the AABB. Use AFTER `canvas_arrange` for fine-tuning. Need 2+ shapes."
    )]
    async fn canvas_align(
        &self,
        Parameters(p): Parameters<CanvasAlignParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if p.shape_ids.len() < 2 {
            return Err(ErrorData::invalid_params("need at least 2 shape ids to align", None));
        }
        validate_one_of(&p.axis, &["left", "center-x", "right", "top", "center-y", "bottom"], "axis")?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Aligning {} shape(s) → {}.",
            p.shape_ids.len(),
            p.axis
        ))]))
    }

    #[tool(
        description = "Equalize gaps between shapes along an axis. The first and last keep their positions; the middle ones are repositioned so consecutive gaps are equal. Use after `canvas_align` for the classic Figma \"align then distribute\" combo. Need 3+ shapes."
    )]
    async fn canvas_distribute(
        &self,
        Parameters(p): Parameters<CanvasDistributeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if p.shape_ids.len() < 3 {
            return Err(ErrorData::invalid_params("need at least 3 shape ids to distribute", None));
        }
        validate_one_of(&p.axis, &["horizontal", "vertical"], "axis")?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Distributing {} shape(s) {} with equal gaps.",
            p.shape_ids.len(),
            p.axis
        ))]))
    }

    #[tool(
        description = "Pan / zoom the canvas viewport programmatically. `x`/`y` are the top-left of the viewport rect in canvas pixels. `zoom` is the scale factor (1.0 = 100%). Use to ZOOM OUT to show the user the whole graph after `canvas_arrange`, or to position the camera on a specific region (use `canvas_focus` for a single shape — it does the centering math for you)."
    )]
    async fn canvas_set_viewport(
        &self,
        Parameters(p): Parameters<CanvasSetViewportParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if !p.x.is_finite() || !p.y.is_finite() {
            return Err(ErrorData::invalid_params("x/y must be finite", None));
        }
        if let Some(z) = p.zoom {
            if !z.is_finite() || z <= 0.0 {
                return Err(ErrorData::invalid_params("zoom must be > 0", None));
            }
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Setting viewport to ({:.0},{:.0}, zoom {}).",
            p.x, p.y,
            p.zoom.map(|z| format!("{:.2}x", z)).unwrap_or_else(|| "current".to_string())
        ))]))
    }

    #[tool(
        description = "Insert an image onto the linked canvas. `base64` is the raw image bytes (PNG / JPEG / GIF / WebP), no data-URL prefix. The image is decoded, sized to its intrinsic dimensions (capped to 480×480), and placed at the optional `(x, y)` (defaulting to viewport center). Use this when you've generated a chart / diagram externally and want to put it on the canvas alongside the user's other content. The base64 should be < 1.5 MB to fit in localStorage; consider downsampling first."
    )]
    async fn canvas_upload_image(
        &self,
        Parameters(p): Parameters<CanvasUploadImageParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if p.base64.is_empty() {
            return Err(ErrorData::invalid_params("`base64` is empty", None));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Inserting an image ({} KB) onto the linked canvas.",
            p.base64.len() * 3 / 4 / 1024
        ))]))
    }

    // ---- Terminal MCP ----------------------------------------------------
    //
    // Woom can host multiple Terminal instances — full PTY-backed
    // shells. These four tools let an agent drive the SAME PTY the
    // user is staring at: keystroke-level visibility into agent work,
    // no parallel-shell drift, no extra surface to switch to. Every
    // call goes through the desktop's localhost bridge (see
    // `terminal_bridge_client`).

    #[tool(
        description = "List every Terminal instance open in Woom. Returns each terminal's human-readable `name` (e.g. `Vermeer`, `Notre-Dame`) — that's what you pass as `id` to terminal_run / terminal_write / terminal_buffer.\n\n**SKIP THIS CALL if your session preamble already shows `linked_to_terminal=<name>`.** When a session is linked to a terminal, the app-instance map in your system prompt already names it — call terminal_run directly with that name. Calling terminal_list anyway is a wasted round-trip.\n\nUse this only when: (a) no link is shown in the preamble and the user asks you to do terminal work, (b) you need to disambiguate between multiple terminals, (c) the user mentions a terminal by name and you want to confirm it exists.\n\nEmpty list = no Terminal instances open yet (suggest the user spawn one from the rail's Terminal popover)."
    )]
    async fn terminal_list(&self) -> Result<CallToolResult, ErrorData> {
        let client = BridgeClient::discover().map_err(bridge_to_mcp)?;
        let resp = client.list().await.map_err(bridge_to_mcp)?;
        if resp.instances.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No Terminal instances open. Ask the user to spawn one from the rail's Terminal popover (long-press the Terminal icon → +)."
                    .to_string(),
            )]));
        }
        let lines: Vec<String> = resp
            .instances
            .iter()
            .map(|i| {
                let name = i.name.as_deref().unwrap_or("(unnamed)");
                let inst = i.instance_id.as_deref().unwrap_or("(none)");
                format!("- name: {}  (instance_id: {}, uuid: {})", name, inst, i.uuid)
            })
            .collect();
        Ok(CallToolResult::success(vec![Content::text(format!(
            "{} terminal{} open:\n{}\n\nPass the `name` (e.g. `Vermeer`, `Notre-Dame`) as the `id` parameter to terminal_run / terminal_write / terminal_buffer — that's the form that reads cleanly in chat history. The `instance_id` (e.g. `terminal-solo`) also resolves; the per-spawn `uuid` is a last-resort handle when two instances share a name.",
            resp.instances.len(),
            if resp.instances.len() == 1 { "" } else { "s" },
            lines.join("\n")
        ))]))
    }

    #[tool(
        description = "Run ONE shell command in a user-visible Terminal instance and block until it finishes. Returns `{ stdout, exit_code, timed_out, interactive_prompt? }`.\n\n## Rules\n\n1. **One purpose per call.** No `;` / `&&` / `for` / multi-pipe blobs. `git status`, then `git diff`, then `git log` — three calls, three responses. The bridge is fast (~50ms round-trip); chaining is a false economy that makes failures impossible to attribute and stdouts impossible to read.\n2. **No `echo '=== separator ==='` lines.** Each call's stdout is already isolated in its own response — separators are noise.\n3. **No pager workarounds.** Pagers (`less`, `more`) are pre-disabled — `PAGER`, `GIT_PAGER`, `GH_PAGER`, `SYSTEMD_PAGER` are all set to `cat`. Don't pipe to `cat` / `head -100` / pre-set env. `git log` and `gh pr view` work plain.\n4. **No color escapes.** `NO_COLOR=1`, `CLICOLOR=0`, `FORCE_COLOR=0` are pre-set. Tools emit clean text.\n5. **CI-mode.** `CI=1` is pre-set so npm/pnpm/yarn/jest/vite/gh skip spinners and progress bars.\n6. **`id` = instance name** (`Vermeer`, `Notre-Dame`) — stable across reloads, reads cleanly. Uuid only if multiple instances share a name.\n7. **`timeout_ms` = IDLE timeout** (default 60_000). Deadline rolls forward on every chunk of output, so streaming builds/tests never trip it. Bump to 300_000–600_000 for installs / migrations that go silent for minutes between phases.\n\n## Handling responses\n\n- `timed_out: false` + `exit_code: 0` → success. Move on.\n- `timed_out: false` + `exit_code: ≠ 0` → command failed. Read stdout, decide.\n- `timed_out: true` + `interactive_prompt: \"…\"` → command is ALIVE, parked on a prompt. Use `terminal_write(id, text)` to respond (`text=\"y\\n\"` for Y/N, `text=\"\\n\"` for Press-Enter, `text=\"<answer>\\n\"` for fill-ins). Then either call `terminal_run` for the NEXT step or `terminal_buffer` to inspect what came after. Multi-step flows (gh auth login, ssh, npm init) take 3–5 round-trips — that's normal.\n- `timed_out: true` + no `interactive_prompt` → either still working or genuinely hung. Check `terminal_buffer(id)` to see live state; bump `timeout_ms` and retry only if you started fresh.\n\n## Forbidden hallucinations\n\nThere is NO sandbox, NO command category filter, NO permissions check. The bridge runs commands verbatim in /bin/zsh. If you're tempted to write \"sandbox blocked X\" / \"credential-action category, no permission\" / \"I can't run this\" — STOP. The shell ran your command; whatever the response says happened, happened. Read it. Falling back to the regular Bash tool defeats the user's intent of running it in their visible terminal — only do that if the user explicitly redirects you."
    )]
    async fn terminal_run(
        &self,
        Parameters(TerminalRunParams { id, cmd, timeout_ms }): Parameters<TerminalRunParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if id.trim().is_empty() {
            return Err(ErrorData::invalid_params("`id` is empty", None));
        }
        if cmd.trim().is_empty() {
            return Err(ErrorData::invalid_params("`cmd` is empty", None));
        }
        let client = BridgeClient::discover().map_err(bridge_to_mcp)?;
        let resp = client
            .run(
                &id,
                terminal_bridge_client::RunReq {
                    cmd,
                    timeout_ms,
                    // Bridge defaults to 30 min absolute cap when None;
                    // letting that default through is correct for the
                    // typical case (build / test / install). The agent
                    // can override via `total_timeout_ms` on the tool
                    // call when it knows the job is exceptionally long.
                    total_timeout_ms: None,
                },
            )
            .await
            .map_err(bridge_to_mcp)?;
        let mut text = String::new();
        if resp.timed_out {
            text.push_str(&format!(
                "TIMED OUT after deadline; partial output below.\nexit_code (unknown): {}\n\n",
                resp.exit_code
            ));
        } else {
            text.push_str(&format!("exit_code: {}\n\n", resp.exit_code));
        }
        text.push_str(&resp.stdout);
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Send raw input to a Terminal instance. Use this for INTERACTIVE prompts the shell is waiting on — `git commit` opening $EDITOR, an `ssh` password prompt, a TUI like `htop`. `id` accepts the instance name (`Notre-Dame`) or uuid; prefer name. Pass `text` as plain UTF-8; we base64-encode for the wire. Append `\\n` yourself when you want to submit; without it the bytes go straight into the line buffer (the user can finish typing).\n\nFor non-interactive command execution prefer `terminal_run` — it captures stdout AND blocks until the command finishes. Use `terminal_write` only when you specifically need to drive an interactive flow."
    )]
    async fn terminal_write(
        &self,
        Parameters(TerminalWriteParams { id, text }): Parameters<TerminalWriteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        use base64::Engine;
        if id.trim().is_empty() {
            return Err(ErrorData::invalid_params("`id` is empty", None));
        }
        let client = BridgeClient::discover().map_err(bridge_to_mcp)?;
        let data_b64 = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
        client
            .write(
                &id,
                terminal_bridge_client::WriteReq { data_b64 },
            )
            .await
            .map_err(bridge_to_mcp)?;
        /* Echo back the instance's art-name rather than the raw `id`
           the agent supplied. Lots of agents pass `terminal-solo` (the
           layout id from the preamble) but the chat reads better with
           the user-visible name (e.g. "Hopper"). Best-effort: if the
           list call fails or finds nothing, fall back to the raw id
           so we never block the response. */
        let label = resolve_terminal_label(&client, &id).await;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Wrote {} byte(s) to terminal {}.",
            text.len(),
            label
        ))]))
    }

    #[tool(
        description = "Read the recent scrollback of a Terminal instance. Returns the last `lines` lines (default 200) of accumulated output, ANSI-stripped. `id` accepts the instance name (`Notre-Dame`) or uuid; prefer name.\n\nUse this to inspect output the user produced themselves (or output from a previous tool call you forgot to capture) — e.g. \"what did the test runner print last?\" or \"what's the user's $PATH?\". Be careful: this returns OLD output that may include prior user attempts. To run a fresh command and capture ONLY its result, use `terminal_run` (the buffer view will include both old and new bytes).\n\n`total_bytes` in the response counts every byte the session has emitted since spawn (mod the 64 KB ring buffer cap)."
    )]
    async fn terminal_buffer(
        &self,
        Parameters(TerminalBufferParams { id, lines }): Parameters<TerminalBufferParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if id.trim().is_empty() {
            return Err(ErrorData::invalid_params("`id` is empty", None));
        }
        let client = BridgeClient::discover().map_err(bridge_to_mcp)?;
        let resp = client.buffer(&id, lines).await.map_err(bridge_to_mcp)?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "(total {} bytes since spawn)\n\n{}",
            resp.total_bytes, resp.text
        ))]))
    }

    // ---- Background tasks --------------------------------------------------
    //
    // Distinct from `terminal_run`: `terminal_run` BLOCKS until the command
    // finishes and is hosted in a user-visible PTY. `bg_spawn` returns
    // immediately, the process stays alive, and the user sees it in the
    // Preview pane on the right of the Claude/Cursor solo. Use bg_* for
    // dev servers, file watchers, test loops, anything long-running. The
    // `bg_wait_line` tool turns the persistent stream into discrete
    // "react when X appears" events without polling.

    #[tool(
        description = "Spawn a long-running background process tracked in the Preview pane. Returns the task `id` immediately — the process keeps running until `bg_kill` is called or it exits on its own. **Use for**: dev servers (`pnpm dev`, `python -m http.server`), file watchers (`cargo watch -x test`), tunneling tools (`ngrok`), any process whose primary value is staying alive and streaming output.\n\n**Do NOT use for** one-shot commands — those go through `terminal_run` (blocks, returns stdout + exit code).\n\nThe response is the freshly-tracked task: `id`, `pid`, `started_at`, `status=running`, `log_path` (rolling 10 MB file on disk). URLs like `http://localhost:5173` printed by the process auto-populate `detected_urls` / `detected_ports` so the Preview pane surfaces them as clickable chips.\n\nAfter spawning, call `bg_wait_line(id, contains=\"Ready\", timeout_ms=30000)` to wait for the readiness signal. Then move on to whatever next step depends on it (e.g. running tests against the server). Logs are reachable via `bg_logs(id, tail=N)` at any time.\n\n`cwd` MUST be absolute. The agent's session preamble shows the worktree path; use that. Spawning without an explicit `cwd` is not supported."
    )]
    async fn bg_spawn(
        &self,
        Parameters(BgSpawnParams { cmd, cwd, label, env }): Parameters<BgSpawnParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if cmd.trim().is_empty() {
            return Err(ErrorData::invalid_params("`cmd` is empty", None));
        }
        if cwd.trim().is_empty() {
            return Err(ErrorData::invalid_params("`cwd` is empty", None));
        }
        let client = BridgeClient::discover().map_err(bridge_to_mcp)?;
        let task = client
            .bg_spawn(terminal_bridge_client::BgSpawnReq {
                cmd: cmd.clone(),
                cwd,
                label,
                session_id: None,
                env,
            })
            .await
            .map_err(bridge_to_mcp)?;
        let url_hint = task
            .detected_urls
            .first()
            .cloned()
            .unwrap_or_else(|| "(no URL detected yet)".to_string());
        Ok(CallToolResult::success(vec![Content::text(format!(
            "spawned bg task `{}` (label=`{}`, pid={}, status=running)\n\
             url: {}\n\
             cmd: {}\n\
             call `bg_wait_line({}, contains=…, timeout_ms=…)` to react to lines, \
             `bg_logs({}, tail=…)` to read scrollback, `bg_kill({})` when done.",
            task.id,
            task.label,
            task.pid.map(|p| p.to_string()).unwrap_or_else(|| "?".to_string()),
            url_hint,
            cmd,
            task.id,
            task.id,
            task.id,
        ))]))
    }

    #[tool(
        description = "List every background task tracked by the Preview pane. Each row shows id, label, status (running / exit code / killed), pid, primary URL if detected, and a tiny tail of recent output. Use this to confirm what's alive before calling `bg_kill`, or to find the `id` of a task you want to `bg_wait_line` on.\n\nReturns newest-first. Empty list = no tasks open (suggest the user spawn one with `bg_spawn` or `/preview <cmd>`)."
    )]
    async fn bg_list(&self) -> Result<CallToolResult, ErrorData> {
        let client = BridgeClient::discover().map_err(bridge_to_mcp)?;
        let tasks = client.bg_list().await.map_err(bridge_to_mcp)?;
        if tasks.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "no background tasks open. spawn one with `bg_spawn`.".to_string(),
            )]));
        }
        let mut out = String::from("id            | label                 | status   | pid    | url\n");
        out.push_str("--------------|-----------------------|----------|--------|----\n");
        for t in &tasks {
            let status = match &t.status {
                terminal_bridge_client::BgStatus::Running => "running".to_string(),
                terminal_bridge_client::BgStatus::Exited { code } => format!("exit {code}"),
                terminal_bridge_client::BgStatus::Killed { reason } => format!("killed:{reason}"),
            };
            let url = t.detected_urls.first().cloned().unwrap_or_else(|| "—".to_string());
            out.push_str(&format!(
                "{:13} | {:21} | {:8} | {:6} | {}\n",
                t.id,
                truncate(&t.label, 21),
                status,
                t.pid.map(|p| p.to_string()).unwrap_or_else(|| "—".into()),
                url
            ));
        }
        Ok(CallToolResult::success(vec![Content::text(out)]))
    }

    #[tool(
        description = "Read the rolling log of a background task. Returns up to the last `tail` lines (default 200) of merged stdout + stderr. Use this to inspect what a dev server / build watcher printed since spawn, or to dig into a crash after a `bg_kill`.\n\nFor LIVE reactions to new lines (e.g. \"wait until the server prints Ready\"), prefer `bg_wait_line` — it long-polls without burning tool round-trips."
    )]
    async fn bg_logs(
        &self,
        Parameters(BgLogsParams { id, tail }): Parameters<BgLogsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if id.trim().is_empty() {
            return Err(ErrorData::invalid_params("`id` is empty", None));
        }
        let client = BridgeClient::discover().map_err(bridge_to_mcp)?;
        let text = client.bg_logs(&id, tail).await.map_err(bridge_to_mcp)?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "Long-poll for the next stdout/stderr line on a background task. Blocks up to `timeout_ms` (capped server-side at 600_000 = 10 min). Returns the matched `BgLine` JSON when a line appears, OR `null` on timeout.\n\nThis is the line-streaming primitive for reacting to a dev server / build / test loop without polling. Use it as: 1) `bg_spawn` the process, 2) `bg_wait_line(id, contains=\"Ready\", timeout_ms=30000)` until you see readiness, 3) move on. For \"watch the build, surface errors\" loops, call repeatedly with `contains=\"error\"` and a moderate timeout.\n\n`contains` is a case-insensitive substring. Pass `null` (or omit) to match the first new line of any content. The bridge filters server-side so you don't pay for irrelevant lines."
    )]
    async fn bg_wait_line(
        &self,
        Parameters(BgWaitLineParams { id, contains, timeout_ms }): Parameters<BgWaitLineParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if id.trim().is_empty() {
            return Err(ErrorData::invalid_params("`id` is empty", None));
        }
        let client = BridgeClient::discover().map_err(bridge_to_mcp)?;
        let line = client
            .bg_wait(
                &id,
                terminal_bridge_client::BgWaitReq { contains, timeout_ms },
            )
            .await
            .map_err(bridge_to_mcp)?;
        match line {
            Some(l) => Ok(CallToolResult::success(vec![Content::text(format!(
                "[{}] {}",
                match l.stream {
                    terminal_bridge_client::BgStream::Stdout => "stdout",
                    terminal_bridge_client::BgStream::Stderr => "stderr",
                },
                l.line
            ))])),
            None => Ok(CallToolResult::success(vec![Content::text(
                "(timeout — no matching line within the requested window)".to_string(),
            )])),
        }
    }

    #[tool(
        description = "Send raw bytes to the stdin of a background task. Use for interactive dev tools that prompt mid-stream (vite hot-reload prompts, `cargo watch` re-run questions, REPLs). Append `\\n` to submit a line; omit to leave bytes in the line buffer.\n\nFor full pty-style interaction (TUI apps like `htop`, `vim`), use `terminal_run`/`terminal_write` instead — bg_* doesn't allocate a pty, so curses-style apps won't render."
    )]
    async fn bg_stdin(
        &self,
        Parameters(BgStdinParams { id, text }): Parameters<BgStdinParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if id.trim().is_empty() {
            return Err(ErrorData::invalid_params("`id` is empty", None));
        }
        let client = BridgeClient::discover().map_err(bridge_to_mcp)?;
        client
            .bg_stdin(&id, terminal_bridge_client::BgStdinReq { text })
            .await
            .map_err(bridge_to_mcp)?;
        Ok(CallToolResult::success(vec![Content::text(
            "stdin written".to_string(),
        )]))
    }

    #[tool(
        description = "Kill a background task (SIGKILL via tokio). Idempotent — calling on an already-exited task is a no-op. The task remains visible in `bg_list` with status `killed:user` so the user can see the lifecycle in the Preview pane."
    )]
    async fn bg_kill(
        &self,
        Parameters(BgIdParams { id }): Parameters<BgIdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if id.trim().is_empty() {
            return Err(ErrorData::invalid_params("`id` is empty", None));
        }
        let client = BridgeClient::discover().map_err(bridge_to_mcp)?;
        client.bg_kill(&id).await.map_err(bridge_to_mcp)?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "killed bg task `{id}`"
        ))]))
    }

    #[tool(
        description = "List every Woom-bundled MCP tool — categories, names, one-line summaries — for orientation when starting an unfamiliar task. Cheap reference call; use when you're not sure which tool surface fits the user's ask (e.g. \"do I have anything for Sentry?\", \"what's the right way to drive the editor?\"). Returns a fixed markdown overview of the 5 bundled sidecars (jira / github / sentry / memory / app) without their per-tool JSON schemas — call the specific tool once you've picked it.\n\nThis is a quick map, NOT lazy schema loading. The CLI already has every schema in context; consider this an index you can scan to avoid re-deriving \"does feature X exist\"."
    )]
    async fn woom_tools_overview(&self) -> Result<CallToolResult, ErrorData> {
        let overview = include_str!("tools_overview.md");
        Ok(CallToolResult::success(vec![Content::text(overview.to_string())]))
    }

    #[tool(
        description = "Ensure a Terminal instance has a live PTY AND link it to the calling session — the one-shot setup the agent needs before driving `terminal_run` / `terminal_write` / `terminal_buffer`.\n\nWhen to call: your session preamble shows no `linked_to_terminal=…`, OR `terminal_list` came back empty, OR the user just asked you to \"run X in the terminal\" and there's no obvious target. This is your bootstrap — call once at the top of the turn, then drive the returned name in subsequent terminal_* tools.\n\nDefaults: with no args, picks the primary terminal instance (the one with `id=terminal-solo` in the preamble, art-name like \"Vermeer\") and links it to YOU (the calling session). The PTY spawns if it wasn't already, cwd inherits from your linked editor (if any) or $HOME.\n\nTargeting a specific instance: pass `instance_name` (the art-name shown in the rail popover — \"Vermeer\", \"Notre-Dame\") OR `instance_id` (e.g. `terminal-solo` or `terminal:vermeer`). Use this when the user already has multiple terminal instances open and named one explicitly.\n\nReturns the instance's art-name as the canonical `id` for follow-up tool calls. Idempotent — calling it twice doesn't spawn twice."
    )]
    async fn ensure_terminal(
        &self,
        Parameters(EnsureTerminalParams { instance_name, instance_id, extras: _ }): Parameters<EnsureTerminalParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let name = instance_name.as_ref().and_then(coerce_to_string);
        let id = instance_id.as_ref().and_then(coerce_to_string);
        let label = name
            .as_deref()
            .or(id.as_deref())
            .unwrap_or("primary terminal instance");
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Ensuring Terminal `{}` is spawned and linked to this session. \
             Use the instance's art-name as the `id` parameter to `terminal_run` / `terminal_write` / \
             `terminal_buffer` from this point on. The PTY spawn races with your next tool call — \
             if `terminal_run` returns \"unknown terminal id-or-name\", retry once after a brief \
             pause; the dispatcher needs ~50–200 ms to finish wiring it up on the first call.",
            label
        ))]))
    }

    #[tool(
        description = "Propose a state-changing shell command (`git switch/merge/push/pull/reset/rebase`, `rm`, `mv`, `npm install`, migrations, deploys, etc.). Surfaces an editable approval card in Woom and BLOCKS until the user approves (command runs) or dismisses. The tool's response is the actual stdout/stderr + exit code — read it and continue this same turn. Read-only commands (git status, ls, cat, grep) use the regular Bash tool, not this one."
    )]
    async fn propose_bash(
        &self,
        Parameters(ProposeBashParams { command, reason }): Parameters<ProposeBashParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = serde_json::json!({
            "command": command,
            "reason": reason,
        });
        let fallback = format!(
            "Bash command queued.\ncommand: {}\nreason: {}",
            command,
            reason.as_deref().unwrap_or("(none)"),
        );
        run_or_fallback("bash", params, fallback).await
    }

    #[tool(
        description = "Ask the user a question and BLOCK until they answer. Surfaces a card in the chat that resolves the tool with their response IN THE SAME TURN, so the agent reasons about the choice and continues without ending the turn.\n\n**Question shapes** (`kind`): pick whichever fits the decision —\n- `single` (default): radio list. Clicking an option auto-submits. Best for short branching choices (A / B / C).\n- `multi`: checkbox list, user picks any combination, clicks Submit. Best for \"which features should I enable\".\n- `text`: free-form input only, no clickable options. Best for \"what should we name this?\", \"paste the secret\", \"describe in one sentence\".\n- `confirm`: Yes / No buttons, no options needed. Best for irreversible-but-recoverable decisions that don't warrant a full propose_bash card.\n\n**Schema**: `question` is the headline. `header` is a 1-2 sentence context blurb above the body. `options` is an array of `{ label, description? }` — 2-4 entries for `single` / `multi`; IGNORED for `text` / `confirm` (omit it). `multi_select=true` is a legacy alias for `kind=multi`.\n\n**When to use**: any branch in your reasoning where the user's preference materially changes what you do next. Use INSTEAD of finishing your turn with a prose question — prose ends the turn and forces a re-context on the user's next message; this tool keeps the same turn alive AND renders interactive UI.\n\n**Don't use** for confirmations of destructive shell / git actions (those go through `propose_bash` / `propose_commit` / `propose_pr` — they ALSO execute). `kind=confirm` here is for \"should I proceed with this refactor / direction?\", not \"should I run `rm -rf`\"."
    )]
    async fn ask_user_question(
        &self,
        Parameters(AskUserQuestionParams { question, header, options, kind, multi_select }): Parameters<AskUserQuestionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if question.trim().is_empty() {
            return Err(ErrorData::invalid_params("`question` is empty", None));
        }
        let effective_kind = kind.unwrap_or(if multi_select == Some(true) {
            AskUserQuestionKind::Multi
        } else {
            AskUserQuestionKind::Single
        });
        let needs_options = matches!(
            effective_kind,
            AskUserQuestionKind::Single | AskUserQuestionKind::Multi
        );
        if needs_options && (options.len() < 2 || options.len() > 4) {
            return Err(ErrorData::invalid_params(
                "`options` must have 2-4 entries for kind=single/multi",
                None,
            ));
        }
        let is_multi = matches!(effective_kind, AskUserQuestionKind::Multi);
        let params = serde_json::json!({
            "question": question,
            "header": header,
            "options": options,
            "kind": effective_kind,
            "multi_select": is_multi,
        });
        let fallback = format!(
            "Question posted (IPC unavailable, user can't click): {}",
            question
        );
        run_or_fallback("question", params, fallback).await
    }

    // ---- SDD orchestrator tools -----------------------------------------
    // Phase 6: surface for an agent driving its own SDD workspace. Each
    // tool is a thin validator — the actual mutation happens on the
    // desktop side, where the frontend's stream parser intercepts the
    // tool_use event, calls the underlying Tauri command, and writes the
    // audit-log row. We deliberately omit `sdd_approve_spec` /
    // `sdd_approve_plan` so the agent CANNOT bypass the user's approval
    // gate — the JSON-RPC layer will return `method-not-found` if it
    // tries.

    #[tool(
        description = "Read the full SDD workspace snapshot (stage, phases, recovery state). Use to refresh your view of what's running, what's done, what's failed. Read-only — no audit cost. Pass `id` from the `linked_to_sdd_phase=<wsid>:<phase>` line in your layout preamble."
    )]
    async fn sdd_get(
        &self,
        Parameters(SddIdParams { id }): Parameters<SddIdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Reading SDD workspace `{id}`. The desktop side will return the full snapshot on the frontend's stream-parser dispatch."
        ))]))
    }

    #[tool(
        description = "List phases in the SDD workspace as a compact array — number, slug, title, status, tasks_done/tasks_total, has_acceptance_passed. Cheap; no audit cost. Use before `sdd_get_phase` to scan which phase is current."
    )]
    async fn sdd_list_phases(
        &self,
        Parameters(SddIdParams { id }): Parameters<SddIdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Listing phases for `{id}`."
        ))]))
    }

    #[tool(
        description = "Read phase `phase`'s body + acceptance results from the workspace. Returns Goal, Context, Tasks, Acceptance criteria, Verification commands, plus the latest acceptance.json (per-check pass/fail/exit_code). Read-only. Call this BEFORE doing the work and BEFORE calling `sdd_log_phase_done`."
    )]
    async fn sdd_get_phase(
        &self,
        Parameters(SddPhaseRefParams { id, phase }): Parameters<SddPhaseRefParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Reading phase {phase} of `{id}`."
        ))]))
    }

    #[tool(
        description = "Read the last `tail` entries (default 50) from phase `phase`'s action log. Useful when investigating a failed phase — the verifier surfaces the last 5 in the failure card, but for full forensics you'll want the longer tail."
    )]
    async fn sdd_get_action_log(
        &self,
        Parameters(SddActionLogParams { id, phase, tail }): Parameters<SddActionLogParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Reading action log of `{id}` phase {phase} (tail={}).",
            tail.unwrap_or(50)
        ))]))
    }

    #[tool(
        description = "Read the result summary + acceptance.json for phase `phase`. Use to inspect what a previously-completed phase actually did, e.g. when picking up a workspace mid-flight or auditing whether a skipped phase left a real result. Read-only."
    )]
    async fn sdd_get_results(
        &self,
        Parameters(SddPhaseRefParams { id, phase }): Parameters<SddPhaseRefParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Reading results of `{id}` phase {phase}."
        ))]))
    }

    #[tool(
        description = "Open the gate for phase `phase` so it transitions PendingApproval → Running. Equivalent to the user clicking 'Start phase'. ONLY call when `sdd_autonomy=semi-auto` is in your context (default mode keeps approval with the user). The previous phase MUST be `done` or `skipped`. `reason` ≥ 5 chars — appended to the audit log so the user can see why you advanced."
    )]
    async fn sdd_advance_phase(
        &self,
        Parameters(SddMutatingPhaseParams { id, phase, reason }): Parameters<SddMutatingPhaseParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        let r = sdd_check_reason(&reason)?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Advancing `{id}` to phase {phase} (reason: {r})."
        ))]))
    }

    #[tool(
        description = "Reset phase `phase` to `pending` so the next advance re-runs it. Use after a verifier failure when you've fixed the root cause and want a clean attempt. Also bumps `retry_count`. ONLY in semi-auto. `reason` ≥ 5 chars."
    )]
    async fn sdd_retry_phase(
        &self,
        Parameters(SddMutatingPhaseParams { id, phase, reason }): Parameters<SddMutatingPhaseParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        let r = sdd_check_reason(&reason)?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Retrying `{id}` phase {phase} (reason: {r})."
        ))]))
    }

    #[tool(
        description = "Mark phase `phase` as `skipped` and persist the reason on the phase frontmatter + meta + audit log. Use ONLY when the phase is genuinely out of scope or blocked by something out of band — never to dodge a verifier failure you could fix. ONLY in semi-auto. `reason` ≥ 5 chars."
    )]
    async fn sdd_skip_phase(
        &self,
        Parameters(SddMutatingPhaseParams { id, phase, reason }): Parameters<SddMutatingPhaseParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        let r = sdd_check_reason(&reason)?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Skipping `{id}` phase {phase} (reason: {r})."
        ))]))
    }

    #[tool(
        description = "Pause the workspace — stops the orchestrator from scheduling new phases until `sdd_resume`. The current phase, if running, finishes naturally. Idempotent. `reason` ≥ 5 chars (e.g. \"need to investigate flaky test before continuing\")."
    )]
    async fn sdd_pause(
        &self,
        Parameters(SddMutatingWorkspaceParams { id, reason }): Parameters<SddMutatingWorkspaceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        let r = sdd_check_reason(&reason)?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Pausing `{id}` (reason: {r})."
        ))]))
    }

    #[tool(
        description = "Resume a paused workspace — clears the pause control file and lets the orchestrator schedule the next phase. Idempotent. `reason` ≥ 5 chars."
    )]
    async fn sdd_resume(
        &self,
        Parameters(SddMutatingWorkspaceParams { id, reason }): Parameters<SddMutatingWorkspaceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        let r = sdd_check_reason(&reason)?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Resuming `{id}` (reason: {r})."
        ))]))
    }

    #[tool(
        description = "Close phase `phase`: orchestrator writes the result summary, runs the verifier, commits to git, and flips the phase frontmatter to `status: done`. THIS IS THE PREFERRED WAY to finish a phase — do NOT manually edit phase frontmatter when this tool is available. `summary` is 2-3 sentences. `files_changed` is the repo-relative paths you touched."
    )]
    async fn sdd_log_phase_done(
        &self,
        Parameters(SddLogPhaseDoneParams { id, phase, summary, files_changed }): Parameters<SddLogPhaseDoneParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        if summary.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "`summary` is empty — write 2-3 sentences describing what observably changed.",
                None,
            ));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Closing `{id}` phase {phase} ({} files changed). Orchestrator will write result.md, run verifier, commit.",
            files_changed.len()
        ))]))
    }

    #[tool(
        description = "Push a milestone marker into phase `phase`'s action log as an `sdd_event` row. Use SPARINGLY for genuinely user-visible milestones inside a long phase (e.g. \"finished refactor of auth module\", \"all tests green\"). Tool-call events already auto-log; don't double-log."
    )]
    async fn sdd_log_action(
        &self,
        Parameters(SddLogActionParams { id, phase, summary, detail }): Parameters<SddLogActionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        let s = summary.trim();
        if s.is_empty() {
            return Err(ErrorData::invalid_params(
                "`summary` is empty — pass a one-line milestone label (≤ 80 chars).",
                None,
            ));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Logging action on `{id}` phase {phase}: {s}{}.",
            detail.as_deref().map(|_| " (with detail)").unwrap_or("")
        ))]))
    }

    #[tool(
        description = "Three-call mode — persist the plan-pass body to `phases/<slug>/plan.md` and advance the substep checkpoint from `Plan` → `Implement` (or stay on `Plan` when `plan_gate=true`). Call this as the FINAL step of the plan pass. `body` is the full markdown plan."
    )]
    async fn sdd_save_phase_plan(
        &self,
        Parameters(SddSavePhasePlanParams { id, phase, body }): Parameters<SddSavePhasePlanParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        if body.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "`body` is empty — write the plan markdown (Approach / Step-by-step / Files / Tests / Risks).",
                None,
            ));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Saved plan for `{id}` phase {phase} ({} chars). Substep advances to Implement (or stays on Plan if plan_gate=true).",
            body.len()
        ))]))
    }

    #[tool(
        description = "Three-call mode — close out the implement pass: advance the substep checkpoint from `Implement` → `Verify` so the next agent turn fires the verify-pass prompt. Persists `summary` + `files_changed` on the phase frontmatter (verify pass quotes them). Phase status STAYS `running` — verify pass is the one that flips done/failed. Call this as the FINAL step of the implement pass."
    )]
    async fn sdd_complete_phase_implement(
        &self,
        Parameters(SddCompletePhaseImplementParams { id, phase, summary, files_changed }): Parameters<SddCompletePhaseImplementParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        if summary.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "`summary` is empty — write 2-3 sentences describing observable changes from the implement pass.",
                None,
            ));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Closing implement pass for `{id}` phase {phase} ({} files changed). Substep advances to Verify; next turn fires verify-pass prompt.",
            files_changed.len()
        ))]))
    }

    #[tool(
        description = "Three-call mode — persist the verify-pass JSON to `phases/<slug>/verify.json`, fill phase frontmatter `summary`, flip phase status to `done` (no deviations) or `failed { trigger: verify_failed }` (deviations present), clear the substep checkpoint. Call this as the FINAL step of the verify pass. `raw_json` is the verdict JSON (markdown fences are tolerated)."
    )]
    async fn sdd_save_phase_verify(
        &self,
        Parameters(SddSavePhaseVerifyParams { id, phase, raw_json }): Parameters<SddSavePhaseVerifyParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        if raw_json.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "`raw_json` is empty — pass the verify-pass JSON verdict (schema: summary, task_compliance, deviations[], notes, files_changed[]).",
                None,
            ));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Saved verify verdict for `{id}` phase {phase} ({} bytes). Phase status will flip done/failed based on deviations.",
            raw_json.len()
        ))]))
    }

    #[tool(
        description = "Three-call mode with `plan_gate=true` — approve the plan-pass output for `phase` and advance the substep checkpoint from `Plan` → `Implement`. Only useful when the workspace's `plan_gate` is enabled. `reason` ≥ 5 chars — audit log line."
    )]
    async fn sdd_approve_phase_plan(
        &self,
        Parameters(SddApprovePhasePlanParams { id, phase, reason }): Parameters<SddApprovePhasePlanParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        let r = sdd_check_reason(&reason)?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Approving plan for `{id}` phase {phase} (reason: {r}). Substep advances to Implement."
        ))]))
    }

    #[tool(
        description = "Three-call mode — discard the plan-pass output for `phase` and mark the phase `failed { trigger: plan_discarded }`. Use when the planned approach is wrong (wrong architecture, missed dependency) and you want a clean retry rather than implementing a flawed plan. `reason` ≥ 5 chars — persisted to phase frontmatter + audit log."
    )]
    async fn sdd_discard_phase_plan(
        &self,
        Parameters(SddDiscardPhasePlanParams { id, phase, reason }): Parameters<SddDiscardPhasePlanParams>,
    ) -> Result<CallToolResult, ErrorData> {
        sdd_check_workspace_id(&id)?;
        let r = sdd_check_reason(&reason)?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Discarding plan for `{id}` phase {phase} (reason: {r}). Phase flips to failed; clear retry path."
        ))]))
    }

    #[tool(
        description = "Propose switching the current session's working directory. Surfaces an approval card in Woom and BLOCKS until the user approves (cwd switches) or dismisses. The tool's response is the actual outcome — react and continue in this same turn."
    )]
    async fn propose_switch_cwd(
        &self,
        Parameters(ProposeSwitchCwdParams { path, reason }): Parameters<ProposeSwitchCwdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = serde_json::json!({
            "path": path,
            "reason": reason,
        });
        let fallback = format!(
            "cwd switch proposal queued.\npath: {}\nreason: {}",
            path,
            reason.as_deref().unwrap_or("(none)"),
        );
        run_or_fallback("switch_cwd", params, fallback).await
    }
}

/// Map a `BridgeError` to the MCP tool error shape. We surface the
/// underlying message verbatim — the agent reads it directly and can
/// suggest concrete user actions ("open a Terminal instance", "restart
/// Woom").
fn bridge_to_mcp(err: terminal_bridge_client::BridgeError) -> ErrorData {
    ErrorData::internal_error(err.to_string(), None)
}

/// Cap a string to `max` chars, replacing the tail with `…` if it
/// overflows. Used by `bg_list` to keep the ASCII table aligned. */
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Resolve any of {instance name, instance_id, uuid} → the human-readable
/// instance name, so MCP tool responses surface "Wrote N bytes to terminal
/// Hopper" instead of "...terminal-solo". Best-effort: returns the raw
/// `id` argument when the bridge can't list (transient) or when no
/// instance matches.
async fn resolve_terminal_label(
    client: &terminal_bridge_client::BridgeClient,
    id: &str,
) -> String {
    if let Ok(list) = client.list().await {
        for inst in list.instances {
            let matches_name = inst.name.as_deref() == Some(id);
            let matches_instance = inst.instance_id.as_deref() == Some(id);
            let matches_uuid = inst.uuid == id;
            if matches_name || matches_instance || matches_uuid {
                if let Some(name) = inst.name {
                    return name;
                }
                return inst.uuid;
            }
        }
    }
    id.to_string()
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EnsureTerminalParams {
    /// Art-name of the terminal instance to ensure (e.g. "Vermeer",
    /// "Notre-Dame"). When omitted, defaults to the primary terminal
    /// instance. Accepts the alias `name`. Same `schemars(with = …)`
    /// trick as `SetEditorRepoPathParams` so cursor-agent doesn't
    /// strip the field.
    #[serde(
        default,
        alias = "name",
        alias = "instanceName",
        alias = "terminal_name",
        alias = "label"
    )]
    #[schemars(with = "Option<String>")]
    instance_name: Option<serde_json::Value>,
    /// Stable id of the terminal instance. Use this when there are
    /// multiple instances and you know the layout id (e.g.
    /// `terminal-solo` for the primary, `terminal:vermeer` for a
    /// secondary). Accepts the alias `id`.
    #[serde(
        default,
        alias = "id",
        alias = "instanceId",
        alias = "terminal_id"
    )]
    #[schemars(with = "Option<String>")]
    instance_id: Option<serde_json::Value>,
    /// Catch-all for wrappers (`args`, `arguments`, …) and aliases we
    /// haven't enumerated. Not currently inspected on the sidecar side
    /// because the dispatcher does its own deep walk for these fields;
    /// we keep the catch-all so deserialize doesn't reject unfamiliar
    /// keys.
    #[serde(flatten)]
    #[allow(dead_code)]
    extras: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TerminalRunParams {
    /// Terminal handle — PREFER the instance's art-name (e.g.
    /// "Vermeer", "Notre-Dame") as shown in `terminal_list` or in
    /// your session preamble's `linked_to_terminal=<name> (id=…)`
    /// line. The layout instance id (e.g. `terminal-solo`,
    /// `terminal:vermeer`) and the per-spawn uuid also resolve as
    /// a fallback. The name reads cleanly in chat history; the
    /// uuid does not.
    id: String,
    /// Shell command (zsh syntax) to run. Wrapped server-side as
    /// `{ <cmd>; }; printf 'sentinel%d\n' $?` so $? captures the
    /// user command's exit, not printf's.
    cmd: String,
    /// Hard deadline for sentinel detection. Default 60_000 (60s);
    /// bump for `cargo build`, `npm install`, etc. The tool returns
    /// `timed_out: true` + partial stdout instead of erroring on
    /// deadline so the agent can decide what to do.
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TerminalWriteParams {
    id: String,
    /// Raw UTF-8 to send. Append `\n` to submit; omit to leave bytes
    /// in the line buffer.
    text: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TerminalBufferParams {
    id: String,
    /// How many trailing lines of the scrollback to return. Default
    /// 200. `0` = whole buffer (capped at ~64 KB by the desktop).
    lines: Option<usize>,
}

// ---- bg_* params ---------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BgSpawnParams {
    /// Shell command to run (sh syntax). Single line — chain with `&&`
    /// only when the steps are genuinely a single logical unit.
    cmd: String,
    /// Absolute working directory. The session's worktree path or repo
    /// root is usually right. Spawning relative paths produces
    /// unreliable behaviour because the bridge has no shell context.
    cwd: String,
    /// Short human-readable label shown in the Preview pane chip. Auto-
    /// derived from the first word of `cmd` when omitted.
    #[serde(default)]
    label: Option<String>,
    /// Optional environment variables layered on top of the parent.
    /// Keys must be ASCII upper-snake. Values pass through verbatim.
    #[serde(default)]
    env: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BgIdParams {
    /// Task id from `bg_spawn` / `bg_list`. Format: `bg-` + 10 hex chars.
    id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BgLogsParams {
    id: String,
    /// Last N lines to return. Default: 200. `0` returns the whole
    /// rolling file (capped at 10 MB).
    tail: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BgWaitLineParams {
    id: String,
    /// Case-insensitive substring the matched line must contain. Omit
    /// to wait for ANY new line (useful for the first "is the server
    /// printing anything yet" probe). Empty string also matches any
    /// line.
    #[serde(default)]
    contains: Option<String>,
    /// How long to block, in ms. Server caps at 600_000 (10 min). The
    /// tool returns `null` on timeout — that's the natural "still
    /// silent, give up" signal; loop with a larger timeout or retry.
    timeout_ms: u64,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
struct AskUserQuestionOption {
    /// Button label — what the user sees + what comes back in the tool
    /// response. Keep short (1-4 words); the description goes below.
    label: String,
    /// Optional muted explanation rendered under the button. Use for
    /// trade-off framing ("faster but uses more memory").
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum AskUserQuestionKind {
    /// Single-choice radio. Clicking an option auto-submits.
    Single,
    /// Multi-select checkboxes. User clicks Submit to confirm.
    Multi,
    /// Free-form text only — no clickable options. `options` is
    /// ignored. Use for "what should we name this?", "paste the
    /// API key", "describe in one sentence what you actually want".
    Text,
    /// Two-button Yes / No prompt. `options` is ignored; Yes / No
    /// labels are rendered by the UI. Result comes back literally
    /// as "Yes" or "No".
    Confirm,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AskUserQuestionParams {
    /// The headline question. Rendered prominently at the card top.
    question: String,
    /// Optional 1-2 sentence context blurb above the option list.
    #[serde(default)]
    header: Option<String>,
    /// Clickable options. 2-4 entries when `kind` is `single` /
    /// `multi`. IGNORED (may be empty) when `kind` is `text` or
    /// `confirm` — those shapes don't render an option list.
    #[serde(default)]
    options: Vec<AskUserQuestionOption>,
    /// Shape of the question. Defaults to `multi` when `multi_select=true`,
    /// else `single`. `text` shows only a free-form input (no buttons);
    /// `confirm` shows Yes / No buttons (no `options` needed).
    #[serde(default)]
    kind: Option<AskUserQuestionKind>,
    /// Legacy alias for `kind=multi`. Prefer `kind` directly in new
    /// call sites; this stays for backwards compat with older agent
    /// turns. Ignored when `kind` is set explicitly.
    #[serde(default)]
    multi_select: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BgStdinParams {
    id: String,
    /// Raw UTF-8 text to write to the child's stdin. Append `\n` to
    /// submit a line; omit to leave bytes in the line buffer (the
    /// child sees the partial input when it next reads).
    text: String,
}

// ---- SDD orchestrator params ---------------------------------------------
// Phase 6: shape the MCP surface that lets an agent drive its own SDD
// workflow. Read-tools (`sdd_get*`) are pure observability; mutating
// tools (`sdd_advance_phase` etc.) require a `reason: String` ≥ 5 chars
// after trim, and the frontend stream parser writes the corresponding
// audit-log entry on every successful call. `sdd_approve_spec` /
// `sdd_approve_plan` are NOT exposed — those gates are user-only.

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SddIdParams {
    /// SDD workspace id (e.g. `sdd-ec99a5aeae`). Pull it out of the
    /// `linked_to_sdd_phase=<wsid>:<phase>` line in the layout snapshot
    /// at the top of your system prompt.
    id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SddPhaseRefParams {
    id: String,
    /// Phase number, 1-indexed.
    phase: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SddActionLogParams {
    id: String,
    phase: u32,
    /// Tail length — most-recent N entries. Default 50.
    #[serde(default)]
    tail: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SddMutatingPhaseParams {
    id: String,
    phase: u32,
    /// Why you're advancing / retrying / skipping. Required, ≥ 5 chars
    /// after trim. The audit log persists this verbatim — write a real
    /// sentence ("verifier flaked on flaky test, retrying once" ⩾
    /// "ok"). Min length is enforced server-side, not just here.
    reason: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SddMutatingWorkspaceParams {
    id: String,
    /// Why. Same rules as `SddMutatingPhaseParams::reason`.
    reason: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SddLogPhaseDoneParams {
    id: String,
    phase: u32,
    /// 2-3 sentence summary of what changed during this phase, in the
    /// same shape the result.md `## Summary` section expects.
    summary: String,
    /// Repo-relative paths the phase touched. Used by the auto-generated
    /// result file's `files_changed:` frontmatter list.
    #[serde(default)]
    files_changed: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SddLogActionParams {
    id: String,
    phase: u32,
    /// Free-form milestone label (≤ 80 chars). Shows up in the live
    /// action feed as a bold `sdd_event` row so the user can see what
    /// the agent calls a "milestone" mid-phase.
    summary: String,
    #[serde(default)]
    detail: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SddSavePhasePlanParams {
    id: String,
    phase: u32,
    /// Markdown body of the plan — written verbatim to
    /// `phases/<slug>/plan.md`. Should mirror the Plan template
    /// from the plan-pass prompt (Approach / Step-by-step / Files
    /// to touch / Tests / Risks).
    body: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SddCompletePhaseImplementParams {
    id: String,
    phase: u32,
    /// 2-3 sentence summary of observable changes — written to the
    /// phase frontmatter so the verify pass can quote it.
    summary: String,
    /// Repo-relative paths the implement-pass touched.
    #[serde(default)]
    files_changed: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SddSavePhaseVerifyParams {
    id: String,
    phase: u32,
    /// Raw JSON output from the verify-pass agent. Backend
    /// `VerifyOutput::parse_or_fallback` tolerates markdown fences
    /// and falls back to a deviations sentinel on parse failure.
    raw_json: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SddApprovePhasePlanParams {
    id: String,
    phase: u32,
    /// Why the plan is approved. Audit-log line. ≥ 5 chars.
    reason: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SddDiscardPhasePlanParams {
    id: String,
    phase: u32,
    /// Why the plan is discarded. Persisted to phase frontmatter +
    /// audit log as `plan_discarded` trigger. ≥ 5 chars.
    reason: String,
}

/// Shared reason-validation gate for the mutating SDD tools. Mirrors the
/// server-side `sdd::audit::validate_reason` so the agent gets a clean
/// error message before the request even touches the desktop process.
fn sdd_check_reason(reason: &str) -> Result<String, ErrorData> {
    let trimmed = reason.trim();
    if trimmed.len() < 5 {
        return Err(ErrorData::invalid_params(
            format!(
                "`reason` is too short ({} chars after trim) — explain why you're \
                 mutating the SDD workflow. Min 5 chars. Examples: \"verifier \
                 flaked, retrying\", \"manual smoke confirmed phase complete\", \
                 \"user asked to skip — out of scope\".",
                trimmed.len()
            ),
            None,
        ));
    }
    Ok(trimmed.to_string())
}

/// Sanity-check the workspace id looks like an SDD id (`sdd-` + ≥ 6 hex
/// chars). Cheap defense against the agent passing a stage name or the
/// repo path by mistake. Best-effort — the actual existence check
/// happens on the desktop side when the Tauri command resolves the
/// registry.
fn sdd_check_workspace_id(id: &str) -> Result<(), ErrorData> {
    let trimmed = id.trim();
    if !trimmed.starts_with("sdd-") || trimmed.len() < "sdd-".len() + 6 {
        return Err(ErrorData::invalid_params(
            format!(
                "`id` does not look like an SDD workspace id (expected `sdd-XXXXXX…`, \
                 got `{trimmed}`). The id appears in your layout preamble as \
                 `linked_to_sdd_phase=<wsid>:<phase>`."
            ),
            None,
        ));
    }
    Ok(())
}

#[tool_handler]
impl ServerHandler for App {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.instructions = Some(
            "Drive Woom's UI directly. Woom is organised as full-screen solos (one app at a time, switched via the rail). Use these tools when the user wants you to NAVIGATE the app on their behalf:\n\
             \n\
             ## Detail panes (slide-over from any solo)\n\
             - open_github_pr / open_github_issue — show a PR or issue in the detail pane (with optional tab focus for PRs).\n\
             - open_jira_issue — open a Jira ticket's slide-over.\n\
             - open_sentry_issue — open a Sentry issue's slide-over.\n\
             \n\
             ## Top-level navigation (switch solos)\n\
             - switch_view — change the active solo (home / github / jira / sentry / claude / cursor / editor / canvas / terminal / rules / library / connections / settings). Use ONLY when the user wants the bare solo without any specific scope; if they name a repo / project / sprint / Sentry filter, prefer the targeted opener below.\n\
             - open_github_repo — GitHub solo + specific repo on a section (code/pulls/issues/actions/releases). Pass `path` with `section=code` to drill into a file (\"open src/lib/auth.ts in efficiently\").\n\
             - open_jira_tab — Jira solo + Jira filters (project_key / status_name / search / board_ids / sprint_ids). Use for \"my Jira tickets in DEVOPS\", \"sprint 160 in Jira\".\n\
             - open_sentry_tab — Sentry solo + Sentry filters (projects / search / status / level / environment). Use for \"production crashes\", \"unresolved errors in checkout-web\".\n\
             \n\
             ## App instances\n\
             Solos can host multiple instances (editor / canvas / terminal) — each with a curated art-name (\"Vermeer\", \"Hopper\", \"Sagrada-Familia\"). The rail's icon for that kind shows a popover listing them. Singleton solos (github / jira / sentry / claude / cursor) always have exactly one instance.\n\
             - add_app_instance — spawn a NEW instance (kind=editor/canvas/terminal). For editor, optionally pass `repo_path` to open a folder immediately. Singleton kinds just switch the rail to that solo. Use ONLY when the user explicitly asks for a new/another instance. Do NOT use for \"switch the editor to /path\" — that's set_editor_repo_path.\n\
             - set_editor_repo_path — change an EXISTING editor instance's open folder. Linked agents auto-follow. CANONICAL shape: `{\"instance_name\": \"<art-name>\", \"repo_path\": \"/abs/path\"}`. The handler is permissive: aliases accepted (`path`, `folder`, `directory`, `cwd`, `repo`, `repoPath`, `folderPath`, `dirPath`, `fullPath`, `absolutePath` for the path; `name`, `instanceName` for the name; `id`, `instanceId`, `uuid` for the id), and the whole arguments object can be wrapped under `args` / `arguments` / `params` / `input`.\n\
             - set_agent_cwd — change an agent session's cwd. `target=self` for yourself, or `instance_name` for another agent instance. `repo_path` accepts the same aliases and wrapper shapes as set_editor_repo_path. Effective from the next turn.\n\
             - list_instances — re-read the app-instance map if you think your preamble is stale.\n\
             - set_github_instance / set_jira_instance / set_sentry_instance — patch filters on an EXISTING source instance (identify by `instance_name` or `instance_id`). Pass only the keys you want to change; omitted keys are preserved. Pass empty string `\"\"` to clear a single-value filter (e.g. `repo=\"\"` = all repos). Use these to retarget an instance the user already has open.\n\
             \n\
             ## Sources\n\
             - open_connect_modal — surface the connect/status modal for any source/agent (use when the user asks about an integration that isn't connected yet — e.g. they mention Slack and you can see it's not in their connected list).\n\
             \n\
             ## Canvas (whiteboard) — only when the session is linked to a canvas\n\
             A linked canvas is announced in your system prompt with the canvas id, dimensions, and a compact shape inventory. ONLY use `canvas_*` tools when this preamble is present — otherwise the tools have nothing to target.\n\
             - canvas_add_shape / canvas_add_shapes — place new shapes (provide `shape_id` if you'll connect it next so you don't round-trip).\n\
             - canvas_update_shape — patch x/y/w/h/rot/props/label of a shape.\n\
             - canvas_delete_shape — remove shape(s); connected edges cascade.\n\
             - canvas_add_edge / canvas_add_edges / canvas_delete_edge — draw / drop connectors. PREFER `canvas_add_edges` (batch) over multiple `canvas_add_edge` calls when wiring up a flowchart — one ⌘Z entry, fewer round-trips. Required: `from_shape_id` + `to_shape_id` (aliases accepted: `from`/`to`/`source`/`target`/`fromId`/`toId`/`fromShapeId`/`toShapeId`/`src`/`dst`/`start`/`end`). Default `from_anchor=mr` + `to_anchor=ml` reads as left-to-right flow.\n\
             - canvas_arrange — auto-layout (dagre / grid / row / column). Run AFTER add_shapes so you don't have to position by hand.\n\
             - canvas_focus — smooth-pan the viewport onto a shape so the user sees what you just added.\n\
             - canvas_set_z — reorder z-stack (to-front / to-back / forward / backward) when shapes overlap.\n\
             - canvas_duplicate — clone shapes at an offset; the clones become the new selection.\n\
             - canvas_find — substring-search labels / text / source / live-card keys when the inventory is too long to scan.\n\
             - canvas_group / canvas_ungroup — wrap shapes in a frame so they move together; ungroup frees them.\n\
             - canvas_lock — freeze a shape's position so subsequent patches ignore it (useful for reference cards).\n\
             - canvas_align / canvas_distribute — align selection on an axis or equalize gaps (Figma's \"align then distribute\" combo).\n\
             - canvas_set_viewport — pan/zoom the camera programmatically (use to zoom out after a layout so the user sees the whole graph).\n\
             - canvas_upload_image — paste a base64-encoded image onto the canvas; useful when you've generated a chart externally.\n\
             \n\
             ## Terminal — drive the user's PTY instance\n\
             Woom can host multiple Terminal instances (real /bin/zsh PTY). The user SEES every keystroke in real time — so prefer these over the generic `bash` tool whenever transparency / debuggability matters. EVERY terminal_* tool's `id` parameter accepts EITHER the instance's art-name (`Notre-Dame`), the layout instance id (`terminal-solo`), or the per-spawn uuid — PREFER NAME so the call reads cleanly in chat history.\n\
             - ensure_terminal(instance_name?, instance_id?) — BOOTSTRAP: ensures a terminal instance has a live PTY AND links it to your session. Call this FIRST when your preamble shows no `linked_to_terminal=…` or when `terminal_list` is empty. Returns the instance's art-name to use as `id` for follow-up tool calls. Default targets the primary terminal instance.\n\
             - terminal_list — discover open terminals (returns name + uuid pairs). SKIP if your preamble already shows `linked_to_terminal=<name> (id=…)` — use that name directly.\n\
             - terminal_run(id, cmd, timeout_ms?) — BLOCKS on a command, returns stdout + exit_code. Stdout is ONLY this run's output (echoes of input + prior scrollback are excluded). Default timeout 60s; use 180000–600000 ms for build / install / test commands. Treat `timed_out: true` as inconclusive — don't decide failure on timeout alone.\n\
             - terminal_write(id, text) — raw input for INTERACTIVE prompts (git editor, ssh password, htop keys). Append \\n to submit.\n\
             - terminal_buffer(id, lines?) — read recent scrollback (default last 200 lines). NOTE: includes prior user input AND prior tool runs — don't infer command results from buffer bytes; use terminal_run to actually run + capture cleanly.\n\
             \n\
             # When to chain calls\n\
             These tools compose. \"Open the actions tab for efficiently\" → open_github_repo(owner=…, repo=efficiently, section=actions) — one call, no need to switch_view first. \"Open another editor on /Users/me/Repos/foo\" → add_app_instance(kind=editor, repo_path=…). Don't ask for confirmation — these are harmless navigation that gives the user the same view they'd get clicking through manually."
                .to_string(),
        );
        info
    }
}

/// Send an action card request to Woom over the action-IPC Unix socket
/// and BLOCK until the user resolves the card.
async fn ipc_request_card(
    kind: &str,
    params: serde_json::Value,
) -> anyhow::Result<(bool, String)> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let socket = std::env::var("WOOM_IPC_SOCKET")
        .context("WOOM_IPC_SOCKET not set")?;
    if socket.trim().is_empty() {
        anyhow::bail!("WOOM_IPC_SOCKET is empty");
    }
    let session_id = std::env::var("WOOM_SESSION_ID")
        .context("WOOM_SESSION_ID not set")?;
    let wait_id = uuid::Uuid::new_v4().to_string();

    let req = serde_json::json!({
        "session_id": session_id,
        "wait_id": wait_id,
        "kind": kind,
        "params": params,
    });
    let mut body = serde_json::to_string(&req)?;
    body.push('\n');

    let stream = UnixStream::connect(&socket)
        .await
        .with_context(|| format!("connect to action-IPC socket at {}", socket))?;
    let (read_half, mut write_half) = stream.into_split();
    write_half.write_all(body.as_bytes()).await?;
    write_half.flush().await?;
    let mut reader = BufReader::new(read_half);
    let mut response_line = String::new();
    let _ = reader.read_line(&mut response_line).await?;
    let resp: serde_json::Value = serde_json::from_str(response_line.trim())
        .with_context(|| format!("parse IPC response: {:?}", response_line))?;
    let ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let summary = resp
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("(no summary returned)")
        .to_string();
    Ok((ok, summary))
}

/// Turn the IPC outcome (or fallback on IPC unavailability) into an MCP
/// CallToolResult. Failures are surfaced in the success text so the agent
/// sees and reasons about every outcome rather than getting an opaque error.
async fn run_or_fallback(
    kind: &str,
    params: serde_json::Value,
    fallback_summary: String,
) -> Result<CallToolResult, ErrorData> {
    match ipc_request_card(kind, params).await {
        Ok((_ok, summary)) => Ok(CallToolResult::success(vec![Content::text(summary)])),
        Err(e) => {
            let msg = format!(
                "{}\n\n(Action IPC unavailable: {}. The card was not registered with Woom's UI.)",
                fallback_summary, e
            );
            Ok(CallToolResult::success(vec![Content::text(msg)]))
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = App::new();
    let service = app.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Parse a JSON value into `SetEditorRepoPathParams` as if it
    /// arrived from the LLM, then run the same extraction logic the
    /// real handler runs. Returns `(repo_path, instance_name,
    /// instance_id)` so the test can assert end-to-end resolution.
    fn parse_editor(value: serde_json::Value) -> (Option<String>, Option<String>, Option<String>) {
        let p: SetEditorRepoPathParams = serde_json::from_value(value).expect("must deserialize");
        let repo = extract_repo_path(&p.repo_path, &p.extras);
        let name = extract_typed_or_recursive(&p.instance_name, &p.extras, INSTANCE_NAME_KEYS);
        let id = extract_typed_or_recursive(&p.instance_id, &p.extras, INSTANCE_ID_KEYS);
        (repo, name, id)
    }

    #[test]
    fn happy_path_canonical_keys() {
        let v = json!({
            "instance_name": "Sagrada-Familia",
            "repo_path": "/Users/me/Repos/foo",
        });
        let (repo, name, id) = parse_editor(v);
        assert_eq!(repo.as_deref(), Some("/Users/me/Repos/foo"));
        assert_eq!(name.as_deref(), Some("Sagrada-Familia"));
        assert!(id.is_none());
    }

    #[test]
    fn alias_path_instead_of_repo_path() {
        let v = json!({"name": "Sagrada-Familia", "path": "/x"});
        let (repo, name, _) = parse_editor(v);
        assert_eq!(repo.as_deref(), Some("/x"));
        assert_eq!(name.as_deref(), Some("Sagrada-Familia"));
    }

    #[test]
    fn wrapped_in_args() {
        // cursor-agent has been observed nesting the whole arguments
        // payload under `args`. The handler must search recursively.
        let v = json!({"args": {"instance_name": "Raphael", "repo_path": "/y"}});
        let (repo, name, _) = parse_editor(v);
        assert_eq!(repo.as_deref(), Some("/y"));
        assert_eq!(name.as_deref(), Some("Raphael"));
    }

    #[test]
    fn wrapped_in_arguments_with_aliased_keys() {
        let v = json!({"arguments": {"name": "Mona-Lisa", "folder": "/z"}});
        let (repo, name, _) = parse_editor(v);
        assert_eq!(repo.as_deref(), Some("/z"));
        assert_eq!(name.as_deref(), Some("Mona-Lisa"));
    }

    #[test]
    fn repo_path_as_array() {
        // A single-element array still resolves to its only string.
        let v = json!({"name": "Raphael", "repo_path": ["/a/b"]});
        let (repo, _, _) = parse_editor(v);
        assert_eq!(repo.as_deref(), Some("/a/b"));
    }

    #[test]
    fn repo_path_as_object_with_path_key() {
        let v = json!({"name": "Raphael", "repo_path": {"path": "/p"}});
        let (repo, _, _) = parse_editor(v);
        assert_eq!(repo.as_deref(), Some("/p"));
    }

    #[test]
    fn empty_string_repo_path_falls_back_to_extras() {
        // Empty string in the canonical slot should not block a
        // recursive lookup elsewhere — the LLM sometimes ships both.
        let v = json!({"name": "Raphael", "repo_path": "", "args": {"path": "/q"}});
        let (repo, _, _) = parse_editor(v);
        assert_eq!(repo.as_deref(), Some("/q"));
    }

    #[test]
    fn missing_repo_path_returns_none() {
        let v = json!({"name": "Raphael"});
        let (repo, _, _) = parse_editor(v);
        assert!(repo.is_none());
    }

    #[test]
    fn instance_id_via_uuid_alias() {
        let v = json!({"uuid": "abc-123", "path": "/r"});
        let (repo, _, id) = parse_editor(v);
        assert_eq!(repo.as_deref(), Some("/r"));
        assert_eq!(id.as_deref(), Some("abc-123"));
    }

    #[test]
    fn double_wrapped_args() {
        // `{"args":{"args":{...}}}` — yes, this happens.
        let v = json!({"args": {"args": {"name": "Raphael", "path": "/s"}}});
        let (repo, name, _) = parse_editor(v);
        assert_eq!(repo.as_deref(), Some("/s"));
        assert_eq!(name.as_deref(), Some("Raphael"));
    }

    /// Regression guard: every field on `SetEditorRepoPathParams` and
    /// `SetAgentCwdParams` MUST advertise a non-empty `"type"` in its
    /// JSON Schema. cursor-agent's tool-binder silently strips fields
    /// whose schema lacks `type`, so the LLM call lands argless on
    /// the server (`repo_path=None`) regardless of what the model
    /// emitted. The historical bug shape was typing the fields as
    /// `Option<serde_json::Value>` without `#[schemars(with = …)]`,
    /// which schemars renders as a property with only `description`
    /// + `default` and no `type` key. If you change the typing or
    /// drop the override, this test catches the regression before
    /// users do.
    fn assert_field_has_type<'a>(
        schema: &'a serde_json::Value,
        field: &str,
    ) -> &'a serde_json::Value {
        let prop = schema
            .get("properties")
            .and_then(|p| p.get(field))
            .unwrap_or_else(|| panic!("schema is missing property `{}`", field));
        let ty = prop.get("type").unwrap_or_else(|| {
            panic!(
                "field `{}` has no `type` in its schema (cursor-agent will strip it). prop = {}",
                field, prop
            )
        });
        assert!(
            !ty.is_null(),
            "field `{}` has explicit null `type` (cursor-agent will strip it)",
            field
        );
        ty
    }

    #[test]
    fn schema_advertises_string_type_for_editor_fields() {
        let schema =
            serde_json::to_value(schemars::schema_for!(SetEditorRepoPathParams)).unwrap();
        for f in ["instance_name", "instance_id", "repo_path"] {
            let ty = assert_field_has_type(&schema, f);
            // Must accept "string" — either literally `"string"` or as
            // an entry in the `["string", "null"]` array form schemars
            // emits for `Option<String>`.
            let accepts_string = match ty {
                serde_json::Value::String(s) => s == "string",
                serde_json::Value::Array(arr) => arr.iter().any(|v| v.as_str() == Some("string")),
                _ => false,
            };
            assert!(
                accepts_string,
                "field `{}` doesn't advertise string type (got {})",
                f, ty
            );
        }
    }

    #[test]
    fn schema_advertises_string_type_for_agent_cwd_fields() {
        let schema = serde_json::to_value(schemars::schema_for!(SetAgentCwdParams)).unwrap();
        for f in ["instance_name", "instance_id", "repo_path", "target"] {
            let ty = assert_field_has_type(&schema, f);
            let accepts_string = match ty {
                serde_json::Value::String(s) => s == "string",
                serde_json::Value::Array(arr) => arr.iter().any(|v| v.as_str() == Some("string")),
                _ => false,
            };
            assert!(
                accepts_string,
                "field `{}` doesn't advertise string type (got {})",
                f, ty
            );
        }
    }
}
