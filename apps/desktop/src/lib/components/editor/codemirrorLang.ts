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

function legacy(mode: Parameters<typeof StreamLanguage.define>[0]): Extension {
  return StreamLanguage.define(mode);
}

// Filename-only matches (no extension or special names). Jenkinsfile, Dockerfile, etc.
const FILENAME_MAP: Record<string, () => Extension> = {
  jenkinsfile: () => legacy(groovy),
  dockerfile: () => legacy(dockerFile),
  'docker-compose.yml': () => yaml(),
  'docker-compose.yaml': () => yaml(),
  makefile: () => legacy(shell),
  vagrantfile: () => legacy(ruby),
  rakefile: () => legacy(ruby),
  gemfile: () => legacy(ruby),
  procfile: () => legacy(shell),
  'cmakelists.txt': () => legacy(shell),

  /* Dotfiles without a real extension. Most of these are tiny config
     files where ALL the user really needs is comment + key=value
     highlighting; shell mode covers `# comment` and bare-word lines
     cleanly enough that a plain `.gitignore` stops looking like a
     wall of white text. JSON-shaped dotfiles get the json mode so
     strings / numbers / brackets light up. */
  '.gitignore': () => legacy(shell),
  '.dockerignore': () => legacy(shell),
  '.prettierignore': () => legacy(shell),
  '.eslintignore': () => legacy(shell),
  '.npmignore': () => legacy(shell),
  '.gitattributes': () => legacy(shell),
  '.editorconfig': () => legacy(toml),
  '.npmrc': () => legacy(shell),
  '.nvmrc': () => legacy(shell),
  '.yarnrc': () => legacy(shell),
  '.tool-versions': () => legacy(shell),
  '.python-version': () => legacy(shell),
  '.ruby-version': () => legacy(shell),
  '.node-version': () => legacy(shell),
  '.env': () => legacy(shell),
  '.envrc': () => legacy(shell),
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
  toml: () => legacy(toml),
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
  groovy: () => legacy(groovy),
  gradle: () => legacy(groovy),
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
  rb: () => legacy(ruby),
  swift: () => legacy(swift),
  // Shells & scripts
  sh: () => legacy(shell),
  bash: () => legacy(shell),
  zsh: () => legacy(shell),
  fish: () => legacy(shell),
  ps1: () => legacy(powerShell),
  psm1: () => legacy(powerShell),
  // SQL
  sql: () => sql(),
  // Niche but useful
  lua: () => legacy(lua),
  pl: () => legacy(perl),
  pm: () => legacy(perl),
  r: () => legacy(r),
  clj: () => legacy(clojure),
  cljs: () => legacy(clojure),
  hs: () => legacy(haskell),
  scm: () => legacy(scheme),
  // Env / config without a syntax (we still highlight comments via shell).
  env: () => legacy(shell),
  conf: () => legacy(shell),
  ini: () => legacy(toml),
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
  if (lower.startsWith('dockerfile.')) return legacy(dockerFile);
  if (lower.startsWith('jenkinsfile.')) return legacy(groovy);

  /* `.env.example`, `.env.local`, `.env.production`, `.env.test`, …
     All are env files in syntax — same shell-style `# comment` +
     `KEY=value`. Catch the family before the generic ext lookup so
     `.env.example` doesn't fall through as an unknown `example`
     extension. */
  if (lower.startsWith('.env.')) return legacy(shell);

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
