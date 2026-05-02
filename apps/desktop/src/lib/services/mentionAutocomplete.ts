/* @-mention autocomplete primitives.
 *
 * The chat composer in AgentColumn renders a textarea with a backdrop
 * div behind it: the backdrop carries highlighted spans for known
 * @tokens (Jira keys, GitHub PRs, Sentry short ids, attached files,
 * paths) while the textarea sits transparently on top contributing the
 * caret and focus ring. The popover suggests candidates for the
 * `@<query>` the user is currently typing.
 *
 * Everything here is pure: helpers operate on plain data + a ranked
 * inbox snapshot the caller provides. No reactive runes, no IPC. The
 * composer wires these into `$derived` blocks. */

import type { InboxItem, JiraItem, SentryIssue } from '$lib/data';

/** A candidate row in the mention popover. The composer maps a click
 *  on this back to a `Mention` on the active session, plus a textarea
 *  edit that swaps the live `@<query>` for `@<externalId>`. */
export type MentionCandidate = {
  source: 'jira' | 'github' | 'sentry' | 'file';
  externalId: string;
  title: string;
  hint: string;
  isDir?: boolean;
  absPath?: string;
  /** Sentry-only — compact context (`type: value · culprit · level`)
      baked into the resulting Mention's `body` so the prompt builder
      forwards it to Claude before MCP follow-up calls. */
  sentryBody?: string;
};

/** HTML-escape a string so we can wrap @tokens in spans without letting
 *  `<` / `&` from the user's text turn into real markup. Keeps the
 *  backdrop a faithful mirror of the textarea content. */
export function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => {
    switch (c) {
      case '&': return '&amp;';
      case '<': return '&lt;';
      case '>': return '&gt;';
      case '"': return '&quot;';
      case "'": return '&#39;';
    }
    return c;
  });
}

/** A `@<token>` earns a highlight only when it resolves to something
 *  real. Keeps random strings like `@bla-bla-bla` as plain text so the
 *  chip style stays meaningful. The rules, in priority order:
 *    1. Jira-style key: `DEVOPS-437` / `EFF-21190`
 *    2. GitHub-style shorthand: `#482`
 *    3. Already attached to the session via the popover or drop
 *    4. Any path containing `/` (assume the user is referring to one)
 *    5. Exact match in the current repo's `git ls-files` index
 *  Rule (4) is intentionally permissive — partial paths like `@src/f`
 *  get a chip as soon as the slash appears, so typing doesn't feel
 *  laggy; (5) catches single-segment filenames at repo root like
 *  `@README.md` where no slash is present. */
export function isKnownMention(
  token: string,
  mentions: { externalId: string }[],
  fileSet: Set<string>
): boolean {
  // Single-segment Jira keys (DEVOPS-437) + multi-segment Sentry short
  // ids (CATALOG-API-76, BMS-API-J6). Trailing segment alphanumeric so
  // base-32-suffix Sentry ids match too.
  if (/^[A-Z][A-Z0-9_]*(?:-[A-Z0-9_]+)+$/.test(token)) return true;
  if (/^#\d+$/.test(token)) return true;
  if (mentions.some((m) => m.externalId === token)) return true;
  if (token.includes('/')) return true;
  if (fileSet.has(token)) return true;
  return false;
}

/** Build the highlighted HTML for the textarea backdrop. Wraps known
 *  `@<token>`s in a span; unknown ones pass through as plain escaped
 *  text. The span intentionally has NO padding / border / margin —
 *  otherwise it would widen the glyphs and the backdrop's line-wrapping
 *  would drift out of sync with the actual textarea wrapping. */
export function highlightMentions(
  text: string,
  mentions: { externalId: string }[],
  fileSet: Set<string>
): string {
  const re = /(^|\s)(@[^\s]+)/g;
  let out = '';
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    const boundary = m[1];
    const tokenFull = m[2]; // includes leading '@'
    const token = tokenFull.slice(1);
    const tokenStart = m.index + boundary.length;
    const tokenEnd = tokenStart + tokenFull.length;
    out += escapeHtml(text.slice(last, m.index));
    out += escapeHtml(boundary);
    if (isKnownMention(token, mentions, fileSet)) {
      out += `<span class="mention-hl">${escapeHtml(tokenFull)}</span>`;
    } else {
      out += escapeHtml(tokenFull);
    }
    last = tokenEnd;
  }
  out += escapeHtml(text.slice(last));
  // Trailing newline: browsers collapse a pure trailing `\n` in white-space:
  // pre-wrap DIVs, so add a zero-width placeholder to keep the backdrop
  // one line taller (matching the textarea's trailing-newline behavior).
  if (out.endsWith('\n')) out += '​';
  return out;
}

/** Rank-one fuzzy score — case-insensitive substring match, with a
 *  big bonus for prefix and a small bonus for contiguous matches near
 *  the start of the string. Good-enough for a composer popover. */
export function scoreMatch(haystack: string, needle: string): number {
  if (!needle) return 1;
  const h = haystack.toLowerCase();
  const n = needle.toLowerCase();
  if (h.startsWith(n)) return 1000 - h.length;
  const idx = h.indexOf(n);
  if (idx < 0) return -1;
  return 500 - idx - h.length;
}

/** Walk back from the caret position: if we're inside a `@token`
 *  (started by whitespace or line-start), return the token shape so
 *  the composer can mount the popover. Returns null when the caret
 *  isn't in a mention context. */
export function readMentionAtCaret(
  value: string,
  caret: number
): { start: number; query: string } | null {
  let i = caret - 1;
  while (i >= 0) {
    const c = value[i];
    if (c === '@') {
      // Require whitespace or start-of-string before the '@' so e.g.
      // an email address isn't mistaken for a mention.
      if (i === 0 || /\s/.test(value[i - 1])) {
        return { start: i, query: value.slice(i + 1, caret) };
      }
      return null;
    }
    if (/\s/.test(c)) return null;
    i--;
  }
  return null;
}

/** Build the ranked candidate list for `@<query>`. Inputs are flat
 *  arrays of items already drawn from every open column, plus the
 *  optional file index for the current repo. Returns the top 12
 *  matches sorted by descending score.
 *
 *  Pure: same inputs ⇒ same output. The composer wraps this in a
 *  `$derived` so it re-runs whenever query / inbox / fileIndex change. */
export function buildMentionCandidates(
  query: string,
  data: {
    jiraItems: JiraItem[];
    githubItems: InboxItem[];
    sentryItems: SentryIssue[];
    fileIndex: { repo: string; paths: string[] } | null;
    activeRepo: string;
  }
): MentionCandidate[] {
  const q = query;
  const out: { cand: MentionCandidate; s: number }[] = [];

  // Jira issues — externalId is the key (e.g. DEVOPS-437).
  for (const j of data.jiraItems) {
    const s = Math.max(scoreMatch(j.key, q), scoreMatch(j.summary, q));
    if (s < 0) continue;
    out.push({
      cand: {
        source: 'jira',
        externalId: j.key,
        title: j.summary,
        hint: `Jira · ${j.status.toLowerCase()}`
      },
      s: s + 10 // small boost: tickets feel most "reference-y"
    });
  }

  // GitHub issues/PRs — externalId is `#<number>` for @mention parity
  // with how the Markdown renderer styles them.
  for (const it of data.githubItems) {
    const id = `#${it.number}`;
    const s = Math.max(scoreMatch(id, q), scoreMatch(it.title, q));
    if (s < 0) continue;
    out.push({
      cand: {
        source: 'github',
        externalId: id,
        title: it.title,
        hint: it.is_pull_request ? 'PR' : 'Issue'
      },
      s
    });
  }

  // Sentry issues — externalId is the short id (e.g. `BMS-API-J6`).
  // `sentryBody` is the compact context block stitched into the Mention
  // so Claude can answer "what's @CATALOG-API-76?" without an MCP
  // round-trip for the basics.
  for (const it of data.sentryItems) {
    const s = Math.max(scoreMatch(it.short_id, q), scoreMatch(it.title, q));
    if (s < 0) continue;
    const bodyParts: string[] = [];
    if (it.metadata_type || it.metadata_value) {
      const t = it.metadata_type ?? '';
      const v = it.metadata_value ?? '';
      bodyParts.push(`${t}${t && v ? ': ' : ''}${v}`.trim());
    }
    if (it.culprit) bodyParts.push(`at ${it.culprit}`);
    bodyParts.push(`level=${it.level}`);
    if (it.project_slug) bodyParts.push(`project=${it.project_slug}`);
    if (it.permalink) bodyParts.push(it.permalink);
    out.push({
      cand: {
        source: 'sentry',
        externalId: it.short_id,
        title: it.title,
        hint: `Sentry · ${it.level}`,
        sentryBody: bodyParts.join(' · ')
      },
      s: s + 8 // small boost — Sentry short-ids are reference-y too
    });
  }

  // Files + folders — filter only when the user has typed at least
  // one char; otherwise the popover would dump the whole repo.
  if (q.length > 0 && data.fileIndex) {
    const repoBase = data.activeRepo.replace(/\/$/, '');
    for (const p of data.fileIndex.paths) {
      const s = scoreMatch(p, q);
      if (s < 0) continue;
      const slash = p.lastIndexOf('/');
      const name = slash >= 0 ? p.slice(slash + 1) : p;
      const dir = slash >= 0 ? p.slice(0, slash) : '';
      out.push({
        cand: {
          source: 'file',
          externalId: p,
          title: name,
          hint: dir || 'file',
          isDir: false,
          absPath: `${repoBase}/${p}`
        },
        s: s - 2 // slight deprioritization vs tickets
      });
    }
  }

  out.sort((a, b) => b.s - a.s);
  return out.slice(0, 12).map((x) => x.cand);
}
