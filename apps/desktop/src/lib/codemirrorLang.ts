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

/**
 * Pick the best CodeMirror language extension for a given file path.
 * Falls back to an empty extension array (plain text) if nothing matches.
 */
export function languageFor(path: string): Extension {
  if (!path) return [];
  const base = path.split('/').pop() ?? path;
  const lower = base.toLowerCase();

  // Exact filename match (Jenkinsfile, Dockerfile, …).
  if (FILENAME_MAP[lower]) return FILENAME_MAP[lower]();

  // Dockerfile.stage, Dockerfile.prod, etc.
  if (lower.startsWith('dockerfile.')) return legacy(dockerFile);
  if (lower.startsWith('jenkinsfile.')) return legacy(groovy);

  const dot = lower.lastIndexOf('.');
  if (dot <= 0) return [];
  const ext = lower.slice(dot + 1);
  return EXT_MAP[ext]?.() ?? [];
}
