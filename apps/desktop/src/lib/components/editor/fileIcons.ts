/* Monochrome file-type icons for the FileTree.
 *
 * Returns inline SVG path data (the `d` attribute body) so the caller
 * can render it inside a `<svg viewBox="0 0 24 24">` and pick its own
 * stroke / fill colour from the tree's CSS — no per-language palette,
 * just shape recognition (matches the user's "icon theme but mono"
 * brief).
 *
 * Lookup order:
 *   1. Folders → folder-open / folder-closed.
 *   2. Special filenames (README, package.json, Dockerfile, …) so e.g.
 *      `package.json` reads as a package, not a generic JSON file.
 *   3. Extension table.
 *   4. Generic file fallback.
 */

export type FileIcon = {
  /** Path body for `<svg viewBox="0 0 24 24"><path d="…"/></svg>`. */
  d: string;
  /** Optional secondary path drawn behind the main one (e.g. an
   *  underline accent for config files). Most icons leave this off. */
  d2?: string;
};

const FOLDER_CLOSED: FileIcon = {
  d: 'M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z'
};
const FOLDER_OPEN: FileIcon = {
  d: 'M3 7v11a2 2 0 0 0 2 2h12l4-9H7a2 2 0 0 0-2 2M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2'
};

/* Generic file (page corner folded). */
const FILE: FileIcon = {
  d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z M14 3v5h5'
};

/* File with code chevrons inside — JS/TS/etc. */
const FILE_CODE: FileIcon = {
  d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z M14 3v5h5',
  d2: 'M9 14l-2 2 2 2M15 14l2 2-2 2M13 13l-2 6'
};

/* File with curly-braces — JSON / config. */
const FILE_JSON: FileIcon = {
  d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z M14 3v5h5',
  d2: 'M10 13c-1 0-1 1-1 2s0 2 -1 2 1 0 1 2 0 2 1 2M14 13c1 0 1 1 1 2s0 2 1 2 -1 0 -1 2 0 2 -1 2'
};

/* File with dash lines — markdown / docs. */
const FILE_DOC: FileIcon = {
  d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z M14 3v5h5',
  d2: 'M9 12h6M9 15h6M9 18h4'
};

/* File with angle brackets — HTML / XML. */
const FILE_MARKUP: FileIcon = {
  d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z M14 3v5h5',
  d2: 'M10 14l-2 2 2 2M14 14l2 2-2 2'
};

/* File with hash — CSS-likes. */
const FILE_STYLE: FileIcon = {
  d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z M14 3v5h5',
  d2: 'M9 13l-1 6M14 13l-1 6M8 15h7M8 17h7'
};

/* File with image-frame — png/jpg/gif/etc. */
const FILE_IMAGE: FileIcon = {
  d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z M14 3v5h5',
  d2: 'M8 18l3-4 2 2 3-3 3 5'
};

/* Wrench-on-page — package.json, lockfiles, tsconfig, configs. */
const FILE_CONFIG: FileIcon = {
  d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z M14 3v5h5',
  d2: 'M10 17l4-4M14 13a2 2 0 0 0 2-2 2 2 0 0 0-2-2M10 13a2 2 0 0 0-2 2 2 2 0 0 0 2 2'
};

/* Container — Dockerfile / docker-compose. */
const FILE_DOCKER: FileIcon = {
  d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z M14 3v5h5',
  d2: 'M8 14h2v2H8zM11 14h2v2h-2zM14 14h2v2h-2zM8 17h8'
};

/* Lock — locks, secrets. */
const FILE_LOCK: FileIcon = {
  d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z M14 3v5h5',
  d2: 'M9.5 15v-1.5a2.5 2.5 0 0 1 5 0V15M8.5 15h7v4h-7z'
};

/* Git — .gitignore, .gitattributes. */
const FILE_GIT: FileIcon = {
  d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z M14 3v5h5',
  d2: 'M9 14a2 2 0 1 1 4 0 2 2 0 0 1-4 0M11 16v3M11 13l3-3'
};

/* Rust — file with crab-ish corner (kept simple, mono). */
const FILE_RUST: FileIcon = {
  d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z M14 3v5h5',
  d2: 'M9 13h4a2 2 0 0 1 0 4h-4zM9 13v6M11 17l2 2'
};

/* Python — page with snake-curve. */
const FILE_PY: FileIcon = {
  d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z M14 3v5h5',
  d2: 'M9 12c0-1 1-1 2-1h2v3h-3a1 1 0 0 0-1 1v2c0 1 1 1 2 1h2'
};

/* Go — page with G. */
const FILE_GO: FileIcon = {
  d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z M14 3v5h5',
  d2: 'M14 13h-3a2 2 0 0 0-2 2v1a2 2 0 0 0 2 2h2v-2'
};

/* SVG — page with vector dot. */
const FILE_SVG: FileIcon = {
  d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z M14 3v5h5',
  d2: 'M9 14l3 5 3-5M11 11h2'
};

const EXT: Record<string, FileIcon> = {
  // Code
  ts: FILE_CODE, tsx: FILE_CODE, mts: FILE_CODE, cts: FILE_CODE,
  js: FILE_CODE, jsx: FILE_CODE, mjs: FILE_CODE, cjs: FILE_CODE,
  svelte: FILE_CODE, vue: FILE_CODE,
  rs: FILE_RUST,
  py: FILE_PY,
  go: FILE_GO,
  java: FILE_CODE, kt: FILE_CODE, scala: FILE_CODE, swift: FILE_CODE,
  c: FILE_CODE, h: FILE_CODE, cpp: FILE_CODE, hpp: FILE_CODE, cc: FILE_CODE,
  rb: FILE_CODE, php: FILE_CODE, lua: FILE_CODE, sh: FILE_CODE, bash: FILE_CODE, zsh: FILE_CODE, fish: FILE_CODE,
  sql: FILE_CODE,

  // Config / data
  json: FILE_JSON, json5: FILE_JSON, jsonc: FILE_JSON,
  yaml: FILE_CONFIG, yml: FILE_CONFIG, toml: FILE_CONFIG, ini: FILE_CONFIG, conf: FILE_CONFIG,
  env: FILE_LOCK,

  // Docs
  md: FILE_DOC, mdx: FILE_DOC, rst: FILE_DOC, txt: FILE_DOC, log: FILE_DOC,

  // Markup / styles
  html: FILE_MARKUP, htm: FILE_MARKUP, xml: FILE_MARKUP,
  css: FILE_STYLE, scss: FILE_STYLE, sass: FILE_STYLE, less: FILE_STYLE, styl: FILE_STYLE,

  // Images
  png: FILE_IMAGE, jpg: FILE_IMAGE, jpeg: FILE_IMAGE, gif: FILE_IMAGE,
  webp: FILE_IMAGE, bmp: FILE_IMAGE, ico: FILE_IMAGE, avif: FILE_IMAGE, tiff: FILE_IMAGE,
  svg: FILE_SVG
};

const SPECIAL: Record<string, FileIcon> = {
  'package.json': FILE_CONFIG,
  'package-lock.json': FILE_LOCK,
  'pnpm-lock.yaml': FILE_LOCK,
  'yarn.lock': FILE_LOCK,
  'cargo.lock': FILE_LOCK,
  'cargo.toml': FILE_CONFIG,
  'go.mod': FILE_CONFIG,
  'go.sum': FILE_LOCK,
  'tsconfig.json': FILE_CONFIG,
  'jsconfig.json': FILE_CONFIG,
  'pnpm-workspace.yaml': FILE_CONFIG,
  'dockerfile': FILE_DOCKER,
  'docker-compose.yml': FILE_DOCKER,
  'docker-compose.yaml': FILE_DOCKER,
  '.gitignore': FILE_GIT,
  '.gitattributes': FILE_GIT,
  '.gitkeep': FILE_GIT,
  '.npmrc': FILE_CONFIG,
  '.nvmrc': FILE_CONFIG,
  '.editorconfig': FILE_CONFIG,
  '.prettierrc': FILE_CONFIG,
  '.eslintrc': FILE_CONFIG,
  '.eslintrc.json': FILE_CONFIG,
  '.eslintrc.js': FILE_CONFIG,
  '.env': FILE_LOCK,
  'readme.md': FILE_DOC,
  'license': FILE_DOC,
  'license.md': FILE_DOC
};

export function iconFor(name: string, isDir: boolean, expanded = false): FileIcon {
  if (isDir) return expanded ? FOLDER_OPEN : FOLDER_CLOSED;
  const lower = name.toLowerCase();
  const special = SPECIAL[lower];
  if (special) return special;
  /* Strip any leading dot first so dotfiles fall through to extension
     (e.g. `.babelrc` → ext `babelrc`) instead of being read as
     extensionless. */
  const trimmed = lower.startsWith('.') ? lower.slice(1) : lower;
  const dotIdx = trimmed.lastIndexOf('.');
  const ext = dotIdx >= 0 ? trimmed.slice(dotIdx + 1) : trimmed;
  return EXT[ext] ?? FILE;
}
