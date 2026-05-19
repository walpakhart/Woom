// Centralized language detection for CodeMirror editors (Editor + DiffView).
// Returns a CodeMirror extension suitable for dropping into a Compartment
// or EditorState.create({ extensions: [...] }).

import type { Extension } from '@codemirror/state';
import { javascript } from '@codemirror/lang-javascript';
import { rust } from '@codemirror/lang-rust';
import { json } from '@codemirror/lang-json';
import { markdown } from '@codemirror/lang-markdown';
import { css } from '@codemirror/lang-css';
import { html } from '@codemirror/lang-html';
import { python } from '@codemirror/lang-python';
import { yaml } from '@codemirror/lang-yaml';
import { xml } from '@codemirror/lang-xml';
import { sql } from '@codemirror/lang-sql';
import { java } from '@codemirror/lang-java';
import { cpp } from '@codemirror/lang-cpp';
import { go } from '@codemirror/lang-go';
import { php } from '@codemirror/lang-php';
import { vue } from '@codemirror/lang-vue';
import { StreamLanguage } from '@codemirror/language';
import { groovy } from '@codemirror/legacy-modes/mode/groovy';
import { shell } from '@codemirror/legacy-modes/mode/shell';
import { dockerFile } from '@codemirror/legacy-modes/mode/dockerfile';
import { ruby } from '@codemirror/legacy-modes/mode/ruby';
import { toml } from '@codemirror/legacy-modes/mode/toml';
import { lua } from '@codemirror/legacy-modes/mode/lua';
import { perl } from '@codemirror/legacy-modes/mode/perl';
import { powerShell } from '@codemirror/legacy-modes/mode/powershell';
import { swift } from '@codemirror/legacy-modes/mode/swift';
import { r } from '@codemirror/legacy-modes/mode/r';
import { clojure } from '@codemirror/legacy-modes/mode/clojure';
import { haskell } from '@codemirror/legacy-modes/mode/haskell';
import { scheme } from '@codemirror/legacy-modes/mode/scheme';

/* StreamLanguage wrapper that lets us inject `commentTokens` for the
 * legacy-mode family. `Mod-/` from CodeMirror's defaultKeymap reads
 * `commentTokens` off the language's languageData facet to decide
 * which prefix to add / strip — without it the keypress is a no-op
 * for every legacy mode (shell, ruby, toml, lua, …). The lang-* npm
 * packages already ship this data; the @codemirror/legacy-modes
 * shipped parsers don't, so we patch it in centrally here. */
type CommentTokens = { line?: string; block?: { open: string; close: string } };
function legacy(
  mode: Parameters<typeof StreamLanguage.define>[0],
  commentTokens?: CommentTokens
): Extension {
  if (!commentTokens) return StreamLanguage.define(mode);
  return StreamLanguage.define({
    ...mode,
    languageData: { ...(mode.languageData ?? {}), commentTokens }
  });
}

/* Comment-token templates by language family. Drives `Mod-/`
 * (toggleLineComment) for every legacy-mode call site below. */
const HASH: CommentTokens = { line: '#' };
const LUA_C: CommentTokens = { line: '--', block: { open: '--[[', close: ']]' } };
const HASKELL_C: CommentTokens = { line: '--', block: { open: '{-', close: '-}' } };
const SLASH_C: CommentTokens = { line: '//', block: { open: '/*', close: '*/' } };
const PS_C: CommentTokens = { line: '#', block: { open: '<#', close: '#>' } };
const SEMI: CommentTokens = { line: ';' };

// Filename-only matches (no extension or special names). Jenkinsfile, Dockerfile, etc.
const FILENAME_MAP: Record<string, () => Extension> = {
  jenkinsfile: () => legacy(groovy, SLASH_C),
  dockerfile: () => legacy(dockerFile, HASH),
  'docker-compose.yml': () => yaml(),
  'docker-compose.yaml': () => yaml(),
  makefile: () => legacy(shell, HASH),
  vagrantfile: () => legacy(ruby, HASH),
  rakefile: () => legacy(ruby, HASH),
  gemfile: () => legacy(ruby, HASH),
  procfile: () => legacy(shell, HASH),
  'cmakelists.txt': () => legacy(shell, HASH),

  /* Dotfiles without a real extension. Most of these are tiny config
     files where ALL the user really needs is comment + key=value
     highlighting; shell mode covers `# comment` and bare-word lines
     cleanly enough that a plain `.gitignore` stops looking like a
     wall of white text. JSON-shaped dotfiles get the json mode so
     strings / numbers / brackets light up. */
  '.gitignore': () => legacy(shell, HASH),
  '.dockerignore': () => legacy(shell, HASH),
  '.prettierignore': () => legacy(shell, HASH),
  '.eslintignore': () => legacy(shell, HASH),
  '.npmignore': () => legacy(shell, HASH),
  '.gitattributes': () => legacy(shell, HASH),
  '.editorconfig': () => legacy(toml, HASH),
  '.npmrc': () => legacy(shell, HASH),
  '.nvmrc': () => legacy(shell, HASH),
  '.yarnrc': () => legacy(shell, HASH),
  '.tool-versions': () => legacy(shell, HASH),
  '.python-version': () => legacy(shell, HASH),
  '.ruby-version': () => legacy(shell, HASH),
  '.node-version': () => legacy(shell, HASH),
  '.env': () => legacy(shell, HASH),
  '.envrc': () => legacy(shell, HASH),
  '.prettierrc': () => json(),
  '.babelrc': () => json(),
  '.eslintrc': () => json(),
  '.stylelintrc': () => json(),
  '.swcrc': () => json(),
};

// Extension-keyed matches (the main path).
const EXT_MAP: Record<string, () => Extension> = {
  // JavaScript / TypeScript family
  js: () => javascript(),
  jsx: () => javascript({ jsx: true }),
  mjs: () => javascript(),
  cjs: () => javascript(),
  ts: () => javascript({ typescript: true }),
  tsx: () => javascript({ typescript: true, jsx: true }),
  // Rust
  rs: () => rust(),
  // Data / config
  json: () => json(),
  jsonc: () => json(),
  yaml: () => yaml(),
  yml: () => yaml(),
  toml: () => legacy(toml, HASH),
  xml: () => xml(),
  svg: () => xml(),
  // Markup / styles / web
  html: () => html(),
  htm: () => html(),
  svelte: () => html(),
  vue: () => vue(),
  css: () => css(),
  scss: () => css(),
  sass: () => css(),
  less: () => css(),
  postcss: () => css(),
  // Docs
  md: () => markdown(),
  markdown: () => markdown(),
  mdx: () => markdown(),
  // Python
  py: () => python(),
  pyi: () => python(),
  // JVM family
  java: () => java(),
  kt: () => java(),     // kotlin is close enough via java highlighter
  kts: () => java(),
  groovy: () => legacy(groovy, SLASH_C),
  gradle: () => legacy(groovy, SLASH_C),
  // C family
  c: () => cpp(),
  h: () => cpp(),
  cc: () => cpp(),
  cpp: () => cpp(),
  cxx: () => cpp(),
  hpp: () => cpp(),
  // Go / PHP / Ruby / Swift
  go: () => go(),
  php: () => php(),
  rb: () => legacy(ruby, HASH),
  swift: () => legacy(swift, SLASH_C),
  // Shells & scripts
  sh: () => legacy(shell, HASH),
  bash: () => legacy(shell, HASH),
  zsh: () => legacy(shell, HASH),
  fish: () => legacy(shell, HASH),
  ps1: () => legacy(powerShell, PS_C),
  psm1: () => legacy(powerShell, PS_C),
  // SQL
  sql: () => sql(),
  // Niche but useful
  lua: () => legacy(lua, LUA_C),
  pl: () => legacy(perl, HASH),
  pm: () => legacy(perl, HASH),
  r: () => legacy(r, HASH),
  clj: () => legacy(clojure, SEMI),
  cljs: () => legacy(clojure, SEMI),
  hs: () => legacy(haskell, HASKELL_C),
  scm: () => legacy(scheme, SEMI),
  // Env / config without a syntax (we still highlight comments via shell).
  env: () => legacy(shell, HASH),
  conf: () => legacy(shell, HASH),
  ini: () => legacy(toml, HASH),
};

/* Strip suffixes that don't change the file's REAL syntax — they only
   mark a variant. `foo.example` / `foo.template` / `foo.sample` /
   `foo.dist` / `foo.bak` / `foo.orig` are all "the foo file but…",
   so peel them off and re-resolve. Bounded to one peel so we don't
   accidentally chase `a.b.c.d` to nothing. */
const VARIANT_SUFFIXES = new Set([
  'example', 'sample', 'template', 'tmpl', 'tpl',
  'dist', 'default', 'bak', 'orig', 'old', 'new', 'in',
]);

/**
 * Pick the best CodeMirror language extension for a given file path.
 * Falls back to an empty extension array (plain text) if nothing matches.
 */
export function languageFor(path: string): Extension {
  if (!path) return [];
  const base = path.split('/').pop() ?? path;
  const lower = base.toLowerCase();

  // Exact filename match (Jenkinsfile, Dockerfile, .gitignore, .env, …).
  if (FILENAME_MAP[lower]) return FILENAME_MAP[lower]();

  // Dockerfile.stage, Dockerfile.prod, etc.
  if (lower.startsWith('dockerfile.')) return legacy(dockerFile, HASH);
  if (lower.startsWith('jenkinsfile.')) return legacy(groovy, SLASH_C);

  /* `.env.example`, `.env.local`, `.env.production`, `.env.test`, …
     All are env files in syntax — same shell-style `# comment` +
     `KEY=value`. Catch the family before the generic ext lookup so
     `.env.example` doesn't fall through as an unknown `example`
     extension. */
  if (lower.startsWith('.env.')) return legacy(shell, HASH);

  /* `.eslintrc.json`, `.babelrc.yaml`, `.prettierrc.toml`, …
     The dotfile basename carries the *family*, the second segment
     carries the *encoding*. Trust the encoding. */
  const dotFirst = lower.indexOf('.', 1);
  if (lower.startsWith('.') && dotFirst > 0) {
    const tail = lower.slice(dotFirst + 1);
    if (EXT_MAP[tail]) return EXT_MAP[tail]();
  }

  const dot = lower.lastIndexOf('.');
  if (dot <= 0) return [];
  const ext = lower.slice(dot + 1);
  if (EXT_MAP[ext]) return EXT_MAP[ext]();

  /* `config.yaml.example`, `service.json.template`, `httpd.conf.dist`,
     `init.sh.bak` — the LAST extension is a variant marker, so peel
     it and try again with the underlying type. Without this every
     `*.example` file rendered as plain text. */
  if (VARIANT_SUFFIXES.has(ext)) {
    const inner = lower.slice(0, dot);
    const innerDot = inner.lastIndexOf('.');
    if (innerDot > 0) {
      const realExt = inner.slice(innerDot + 1);
      if (EXT_MAP[realExt]) return EXT_MAP[realExt]();
    }
  }

  return [];
}
