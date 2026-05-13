//! Library / extension store for Claude. Manages skills (markdown files
//! in `~/.claude/skills/<slug>/SKILL.md`) and plugins (installed via the
//! `claude plugin` CLI subcommand).
//!
//! Phase 1 covers SKILLS end-to-end:
//!   - `library_list_skills()` — read `~/.claude/skills/`, parse the
//!     frontmatter of each `SKILL.md` to surface `name` + `description`.
//!   - `library_install_skill_git(url, slug)` — `git clone <url>
//!     ~/.claude/skills/<slug>`. Caller (frontend) decides the slug
//!     from the catalog entry so two installs of the same skill don't
//!     race on directory creation.
//!   - `library_uninstall_skill(slug)` — `rm -rf ~/.claude/skills/<slug>`.
//!     Refuses paths containing `..` / `/` to keep the wipe local.
//!
//! Plugins (the second half — `claude plugin marketplace add` +
//! `claude plugin install`) are wired with the same shape but shell out
//! to the CLI. We bail with a polite error if `claude` isn't on PATH.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::claude::home_dir;

/// Cache dir for the `anthropics/skills` clone. We keep one shallow
/// checkout and reuse it across installs — installing 5 skills
/// shouldn't trigger 5 full clones.
fn anthropic_skills_cache() -> Option<PathBuf> {
    home_dir().map(|h| h.join("Library/Caches/com.woom.desktop/anthropic-skills"))
}

const ANTHROPIC_SKILLS_REPO: &str = "https://github.com/anthropics/skills.git";
const ANTHROPIC_PLUGINS_MARKETPLACE: &str = "anthropics/claude-plugins-official";

#[derive(Debug, Serialize, Clone)]
pub struct InstalledSkill {
    /// Directory name under `~/.claude/skills/`. Stable identifier; used
    /// by `library_uninstall_skill`.
    pub slug: String,
    /// `name:` from the SKILL.md frontmatter, falling back to `slug`
    /// when the file is missing or malformed.
    pub name: String,
    /// `description:` from the frontmatter, empty when absent. The UI
    /// shows this on the Installed card so the user can recall what
    /// each skill does without opening the file.
    pub description: String,
    /// Absolute path to the skill directory.
    pub path: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct InstalledPlugin {
    /// Plugin name (= directory under `~/.claude/plugins/`).
    pub name: String,
    pub path: String,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct InstalledList {
    pub skills: Vec<InstalledSkill>,
    pub plugins: Vec<InstalledPlugin>,
}

fn skills_dir() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".claude").join("skills"))
}

fn plugins_dir() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".claude").join("plugins"))
}

/// Parse the simple frontmatter Claude skills use:
///   ---
///   name: foo
///   description: ...
///   ---
/// We don't pull in a YAML crate — the format is shallow (top-level
/// `key: value` pairs only, no nested structures) so a hand-rolled
/// scanner is enough.
fn parse_frontmatter(content: &str) -> (Option<String>, Option<String>) {
    let mut lines = content.lines();
    if lines.next().map(|l| l.trim()) != Some("---") {
        return (None, None);
    }
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    for line in lines {
        let t = line.trim();
        if t == "---" {
            break;
        }
        if let Some(rest) = t.strip_prefix("name:") {
            name = Some(rest.trim().trim_matches('"').to_string());
        } else if let Some(rest) = t.strip_prefix("description:") {
            description = Some(rest.trim().trim_matches('"').to_string());
        }
    }
    (name, description)
}

pub fn list_installed() -> InstalledList {
    let mut out = InstalledList::default();
    if let Some(sd) = skills_dir() {
        if let Ok(rd) = std::fs::read_dir(&sd) {
            for entry in rd.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let slug = entry.file_name().to_string_lossy().to_string();
                if slug.starts_with('.') {
                    continue;
                }
                let skill_md = path.join("SKILL.md");
                let (name, description) = std::fs::read_to_string(&skill_md)
                    .ok()
                    .map(|c| parse_frontmatter(&c))
                    .unwrap_or((None, None));
                out.skills.push(InstalledSkill {
                    name: name.unwrap_or_else(|| slug.clone()),
                    description: description.unwrap_or_default(),
                    path: path.display().to_string(),
                    slug,
                });
            }
        }
    }
    if let Some(pd) = plugins_dir() {
        if let Ok(rd) = std::fs::read_dir(&pd) {
            for entry in rd.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                out.plugins.push(InstalledPlugin {
                    name,
                    path: path.display().to_string(),
                });
            }
        }
    }
    out.skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out.plugins.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// Reject slugs that could escape `~/.claude/skills/`. Both `..` and
/// path separators are out — anything more permissive than `[a-z0-9-_]`
/// risks symlink shenanigans.
fn slug_is_safe(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 100
        && slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn install_skill_git(url: &str, slug: &str) -> Result<InstalledSkill, String> {
    if !slug_is_safe(slug) {
        return Err(format!("invalid slug: {slug}"));
    }
    if url.trim().is_empty() {
        return Err("empty git url".into());
    }
    let target = skills_dir().ok_or_else(|| "no HOME".to_string())?.join(slug);
    if target.exists() {
        return Err(format!("already installed at {}", target.display()));
    }
    if let Some(parent) = target.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir -p {}: {}", parent.display(), e))?;
        }
    }
    let out = Command::new("git")
        .arg("clone")
        .arg("--depth=1")
        .arg(url)
        .arg(&target)
        .output()
        .map_err(|e| format!("spawn git: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("git clone failed: {stderr}"));
    }
    let skill_md = target.join("SKILL.md");
    if !skill_md.exists() {
        /* Repo doesn't look like a skill — bail and clean up so the
           Installed list doesn't surface an empty directory. */
        let _ = std::fs::remove_dir_all(&target);
        return Err("repository has no SKILL.md at its root".into());
    }
    let (name, description) = std::fs::read_to_string(&skill_md)
        .ok()
        .map(|c| parse_frontmatter(&c))
        .unwrap_or((None, None));
    Ok(InstalledSkill {
        name: name.unwrap_or_else(|| slug.to_string()),
        description: description.unwrap_or_default(),
        path: target.display().to_string(),
        slug: slug.to_string(),
    })
}

/// Write a one-file skill straight from the catalog blob (when the
/// catalog entry ships the SKILL.md inline rather than pointing at a
/// repo). Useful for tiny one-shot skills we want to bundle without
/// publishing a repo per entry.
pub fn install_skill_inline(slug: &str, content: &str) -> Result<InstalledSkill, String> {
    if !slug_is_safe(slug) {
        return Err(format!("invalid slug: {slug}"));
    }
    let target = skills_dir().ok_or_else(|| "no HOME".to_string())?.join(slug);
    if target.exists() {
        return Err(format!("already installed at {}", target.display()));
    }
    std::fs::create_dir_all(&target)
        .map_err(|e| format!("mkdir -p {}: {}", target.display(), e))?;
    let skill_md = target.join("SKILL.md");
    std::fs::write(&skill_md, content)
        .map_err(|e| format!("write SKILL.md: {e}"))?;
    let (name, description) = parse_frontmatter(content);
    Ok(InstalledSkill {
        name: name.unwrap_or_else(|| slug.to_string()),
        description: description.unwrap_or_default(),
        path: target.display().to_string(),
        slug: slug.to_string(),
    })
}

/// Refresh the cached clone of `anthropics/skills`. First call clones
/// shallow; subsequent calls run `git fetch --depth=1` + reset so the
/// cache stays current without bloating with history. Idempotent.
fn refresh_anthropic_cache() -> Result<PathBuf, String> {
    let cache = anthropic_skills_cache().ok_or_else(|| "no HOME".to_string())?;
    if let Some(parent) = cache.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir -p {}: {}", parent.display(), e))?;
        }
    }
    if cache.join(".git").exists() {
        let out = Command::new("git")
            .args(["fetch", "--depth=1", "origin", "main"])
            .current_dir(&cache)
            .output()
            .map_err(|e| format!("git fetch: {e}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(format!("git fetch failed: {stderr}"));
        }
        let out = Command::new("git")
            .args(["reset", "--hard", "origin/main"])
            .current_dir(&cache)
            .output()
            .map_err(|e| format!("git reset: {e}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(format!("git reset failed: {stderr}"));
        }
    } else {
        let out = Command::new("git")
            .args(["clone", "--depth=1", ANTHROPIC_SKILLS_REPO])
            .arg(&cache)
            .output()
            .map_err(|e| format!("git clone: {e}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(format!("git clone failed: {stderr}"));
        }
    }
    Ok(cache)
}

/// Recursively copy a directory tree. `std::fs` has no `cp -r`, and
/// shelling out gives flaky errors on missing-parent scenarios — this
/// covers all cases we hit (files + nested folders, no symlinks).
fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst)
        .map_err(|e| format!("mkdir {}: {}", dst.display(), e))?;
    for entry in std::fs::read_dir(src)
        .map_err(|e| format!("read_dir {}: {}", src.display(), e))?
        .flatten()
    {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        if meta.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .map_err(|e| format!("copy {} -> {}: {}", from.display(), to.display(), e))?;
        }
    }
    Ok(())
}

/// Install one of the Anthropic-maintained skills from
/// `anthropics/skills/skills/<name>`. The whole skills repo is shallow-
/// cloned to a cache dir on first call; per-install we copy just the
/// requested subfolder into `~/.claude/skills/<name>`.
pub fn install_anthropic_skill(name: &str) -> Result<InstalledSkill, String> {
    if !slug_is_safe(name) {
        return Err(format!("invalid skill name: {name}"));
    }
    let cache = refresh_anthropic_cache()?;
    let source = cache.join("skills").join(name);
    if !source.exists() {
        return Err(format!("skill `{name}` not found in anthropics/skills"));
    }
    let target = skills_dir().ok_or_else(|| "no HOME".to_string())?.join(name);
    if target.exists() {
        return Err(format!("already installed at {}", target.display()));
    }
    copy_dir(&source, &target)?;
    let skill_md = target.join("SKILL.md");
    let (parsed_name, description) = std::fs::read_to_string(&skill_md)
        .ok()
        .map(|c| parse_frontmatter(&c))
        .unwrap_or((None, None));
    Ok(InstalledSkill {
        name: parsed_name.unwrap_or_else(|| name.to_string()),
        description: description.unwrap_or_default(),
        path: target.display().to_string(),
        slug: name.to_string(),
    })
}

pub fn uninstall_skill(slug: &str) -> Result<(), String> {
    if !slug_is_safe(slug) {
        return Err(format!("invalid slug: {slug}"));
    }
    let target = skills_dir().ok_or_else(|| "no HOME".to_string())?.join(slug);
    if !target.exists() {
        return Ok(()); /* idempotent — Installed list already removed it. */
    }
    std::fs::remove_dir_all(&target)
        .map_err(|e| format!("rm -rf {}: {}", target.display(), e))
}

/// Install a plugin from the Anthropic official marketplace. Ensures
/// the `claude-plugins-official` marketplace is registered first
/// (idempotent — Claude's CLI returns an "already added" notice when
/// it's there). Then `claude plugin install <name>@claude-plugins-official`.
pub fn plugin_install_anthropic(name: &str) -> Result<String, String> {
    /* Best-effort marketplace add; if it's already configured the CLI
       just no-ops with an info line. We swallow non-fatal stderr here
       — the install call below is the source of truth on success. */
    let _ = Command::new("claude")
        .args(["plugin", "marketplace", "add", ANTHROPIC_PLUGINS_MARKETPLACE])
        .output();
    let reference = format!("{name}@claude-plugins-official");
    let out = Command::new("claude")
        .args(["plugin", "install", &reference])
        .output()
        .map_err(|e| format!("spawn claude: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("claude plugin install failed: {stderr}"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Run `claude plugin install <ref>`. Caller (frontend) is responsible
/// for adding the marketplace via `library_plugin_marketplace_add` first
/// if the ref isn't already known. Returns stdout on success so the UI
/// can surface what the CLI said.
pub fn plugin_install(reference: &str) -> Result<String, String> {
    let out = Command::new("claude")
        .arg("plugin")
        .arg("install")
        .arg(reference)
        .output()
        .map_err(|e| format!("spawn claude: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("claude plugin install failed: {stderr}"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn plugin_uninstall(name: &str) -> Result<String, String> {
    let out = Command::new("claude")
        .arg("plugin")
        .arg("uninstall")
        .arg(name)
        .output()
        .map_err(|e| format!("spawn claude: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("claude plugin uninstall failed: {stderr}"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn plugin_marketplace_add(url: &str) -> Result<String, String> {
    let out = Command::new("claude")
        .arg("plugin")
        .arg("marketplace")
        .arg("add")
        .arg(url)
        .output()
        .map_err(|e| format!("spawn claude: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("claude plugin marketplace add failed: {stderr}"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_basics() {
        let (n, d) = parse_frontmatter(
            "---\nname: foo\ndescription: does foo things\n---\nbody",
        );
        assert_eq!(n.as_deref(), Some("foo"));
        assert_eq!(d.as_deref(), Some("does foo things"));
    }

    #[test]
    fn frontmatter_missing() {
        let (n, d) = parse_frontmatter("no frontmatter here");
        assert!(n.is_none());
        assert!(d.is_none());
    }

    #[test]
    fn slug_safety() {
        assert!(slug_is_safe("review"));
        assert!(slug_is_safe("my-skill_1"));
        assert!(!slug_is_safe(""));
        assert!(!slug_is_safe(".."));
        assert!(!slug_is_safe("a/b"));
        assert!(!slug_is_safe("../etc"));
    }
}
