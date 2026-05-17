/* Skills — user-defined slash commands that pre-resolve a markdown
 * template (frontmatter + `$ARGUMENTS` + `` !`<cmd>` `` injection) and
 * stamp the result as the next user message. Mirrors `skills.rs`.
 *
 * Skills live on disk under:
 *   - `~/.claude/skills/<name>/SKILL.md`   (user scope)
 *   - `<repo>/.claude/skills/<name>/SKILL.md` (project, walked up
 *     from the agent session's cwd)
 *
 * The store keeps two lists: `userSkills` (cwd-independent) and a
 * cache keyed by cwd for project-scoped skills. Composer reads the
 * union when filtering the slash-picker. */

import { invoke } from '@tauri-apps/api/core';

export type SkillScope = 'user' | 'project';

export interface Skill {
  id: string;          // <scope>:<name>
  name: string;
  description: string | null;
  when_to_use: string | null;
  argument_hint: string | null;
  allowed_tools: string[];
  model: string | null;
  scope: SkillScope;
  path: string;
}

export interface ShellResult {
  cmd: string;
  ok: boolean;
  code: number;
  stdout_truncated: boolean;
}

export interface RenderedSkill {
  skill: Skill;
  rendered: string;
  shell_results: ShellResult[];
}

export const skillsState = $state<{
  /** Combined list (project + user) — recomputed when cwd changes. */
  list: Skill[];
  /** Last cwd we discovered against, so a session switch can trigger
   *  rediscovery without thrashing. */
  lastCwd: string | null;
  /** Discovery in flight. UI hides the slash-picker skill rows while
   *  true to avoid showing a stale list. */
  loading: boolean;
}>({ list: [], lastCwd: null, loading: false });

/** Discover skills for a given cwd. Cheap (Rust just walks a small
 *  number of dirs), but throttled per-cwd so the Composer's $effect
 *  doesn't re-fire on every keystroke that mutates `s.cwd`. */
export async function refreshSkills(cwd: string | null): Promise<void> {
  if (skillsState.lastCwd === cwd && skillsState.list.length > 0) return;
  skillsState.loading = true;
  skillsState.lastCwd = cwd;
  try {
    const list = await invoke<Skill[]>('skills_discover', { cwd: cwd ?? null });
    skillsState.list = list;
  } catch (e) {
    console.warn('skills_discover failed', e);
    skillsState.list = [];
  } finally {
    skillsState.loading = false;
  }
}

/** Render a skill body — substitutes `$ARGUMENTS` + runs shell
 *  injection. Returns the resolved markdown the agent should see plus
 *  per-shell-command results so the UI can hint at failures. */
export async function renderSkill(
  id: string,
  args: string,
  cwd: string | null
): Promise<RenderedSkill | null> {
  try {
    return await invoke<RenderedSkill>('skills_render', {
      id,
      arguments: args,
      cwd: cwd ?? null
    });
  } catch (e) {
    console.warn('skills_render failed', e);
    return null;
  }
}

/** Lookup helper for the Composer slash-picker. Match by name or full
 *  id, case-insensitive. Returns the first project-scoped match
 *  (which always sorts ahead of user-scoped). */
export function findSkill(token: string): Skill | null {
  const t = token.toLowerCase();
  return (
    skillsState.list.find((s) => s.id.toLowerCase() === t) ??
    skillsState.list.find((s) => s.name.toLowerCase() === t) ??
    null
  );
}
