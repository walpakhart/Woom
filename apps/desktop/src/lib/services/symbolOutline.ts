/* Lightweight regex-based symbol outline.
 *
 * Why not tree-sitter (yet): the WASM grammars run a few hundred KB
 * each and need a per-language loader graph that complicates Vite's
 * bundling story. ⇧⌘O is a navigation aid — users hit it dozens of
 * times a day to jump between functions in the file they're already
 * reading. A few well-tested regexes cover ~95% of those jumps for
 * the languages Woom users actually open every day; tree-sitter can
 * land later as a quality bump without changing the surface.
 *
 * Heuristic, not a parser:
 *   - Each detector matches at the start of a line (after indent),
 *     captures the symbol name, and tags a kind. Anything inside a
 *     string / comment that happens to look like a declaration is
 *     accepted — symbol pickers tolerate the occasional false
 *     positive far better than missing real symbols.
 *   - Indentation level is preserved as `depth` for the picker so
 *     class methods render under their owner with a soft tree
 *     glyph; we don't try to truly nest (no scope stack), but
 *     "Python class with two methods" reads correctly anyway.
 *
 * Add a language: drop another entry in `LANGUAGE_DETECTORS`. Each
 * detector is a (regex, kind) pair where the regex MUST capture the
 * symbol name in group 1. Multiple detectors per language are fine —
 * earlier matches win on the same line. */

export type SymbolKind =
  | 'function'
  | 'class'
  | 'interface'
  | 'type'
  | 'enum'
  | 'method'
  | 'variable'
  | 'struct'
  | 'trait'
  | 'mod'
  | 'macro'
  | 'section';

export interface SymbolEntry {
  /** 1-based line number — what the user (and CodeMirror) reads. */
  line: number;
  /** Display name. */
  name: string;
  kind: SymbolKind;
  /** Indent depth in spaces / 2 — drives the tree glyph in the
   *  picker. 0 for top-level. */
  depth: number;
  /** Trimmed line text — used as the secondary display so users
   *  recognise the function signature. */
  preview: string;
}

type Detector = { rx: RegExp; kind: SymbolKind };

/* Common helpers — `\b` boundaries keep us from matching `myFunction`
   when the user wrote `function-ish`. Most languages share the same
   identifier shape so we centralise it. */
const ID = '([A-Za-z_$][\\w$]*)';

const TS_DETECTORS: Detector[] = [
  { rx: new RegExp(`^\\s*export\\s+(?:default\\s+)?(?:async\\s+)?function\\s*\\*?\\s*${ID}`), kind: 'function' },
  { rx: new RegExp(`^\\s*(?:async\\s+)?function\\s*\\*?\\s*${ID}`), kind: 'function' },
  { rx: new RegExp(`^\\s*export\\s+(?:abstract\\s+)?class\\s+${ID}`), kind: 'class' },
  { rx: new RegExp(`^\\s*(?:abstract\\s+)?class\\s+${ID}`), kind: 'class' },
  { rx: new RegExp(`^\\s*export\\s+interface\\s+${ID}`), kind: 'interface' },
  { rx: new RegExp(`^\\s*interface\\s+${ID}`), kind: 'interface' },
  { rx: new RegExp(`^\\s*export\\s+type\\s+${ID}`), kind: 'type' },
  { rx: new RegExp(`^\\s*type\\s+${ID}\\s*=`), kind: 'type' },
  { rx: new RegExp(`^\\s*export\\s+enum\\s+${ID}`), kind: 'enum' },
  { rx: new RegExp(`^\\s*enum\\s+${ID}`), kind: 'enum' },
  /* Arrow / method shorthand. We catch `const foo = (…) =>` and the
     two-line `const foo =\n  () =>` will still surface (we just see
     line one). Standalone `let / var` only when followed by `=` so
     we don't match every variable; users overwhelmingly want
     functions / classes here. */
  { rx: new RegExp(`^\\s*export\\s+(?:const|let|var)\\s+${ID}\\s*[:=]`), kind: 'variable' },
  { rx: new RegExp(`^\\s*(?:const|let|var)\\s+${ID}\\s*[:=]\\s*(?:async\\s*)?(?:function|\\(|\\<)`), kind: 'function' },
  /* Class-body methods (also picks up object methods — close enough). */
  { rx: new RegExp(`^\\s+(?:public|private|protected|static|async|get|set)?\\s*(?:async\\s+)?\\*?\\s*${ID}\\s*\\([^)]*\\)\\s*[{:]`), kind: 'method' }
];

const RUST_DETECTORS: Detector[] = [
  { rx: new RegExp(`^\\s*pub(?:\\([^)]*\\))?\\s+(?:async\\s+)?fn\\s+${ID}`), kind: 'function' },
  { rx: new RegExp(`^\\s*(?:async\\s+)?fn\\s+${ID}`), kind: 'function' },
  { rx: new RegExp(`^\\s*pub\\s+struct\\s+${ID}`), kind: 'struct' },
  { rx: new RegExp(`^\\s*struct\\s+${ID}`), kind: 'struct' },
  { rx: new RegExp(`^\\s*pub\\s+enum\\s+${ID}`), kind: 'enum' },
  { rx: new RegExp(`^\\s*enum\\s+${ID}`), kind: 'enum' },
  { rx: new RegExp(`^\\s*pub\\s+trait\\s+${ID}`), kind: 'trait' },
  { rx: new RegExp(`^\\s*trait\\s+${ID}`), kind: 'trait' },
  { rx: new RegExp(`^\\s*impl(?:<[^>]*>)?\\s+(?:.+\\s+for\\s+)?${ID}`), kind: 'class' },
  { rx: new RegExp(`^\\s*pub\\s+mod\\s+${ID}`), kind: 'mod' },
  { rx: new RegExp(`^\\s*mod\\s+${ID}`), kind: 'mod' },
  { rx: new RegExp(`^\\s*macro_rules!\\s+${ID}`), kind: 'macro' },
  { rx: new RegExp(`^\\s*pub\\s+(?:const|static)\\s+${ID}`), kind: 'variable' },
  { rx: new RegExp(`^\\s*type\\s+${ID}\\s*=`), kind: 'type' }
];

const PY_DETECTORS: Detector[] = [
  { rx: new RegExp(`^\\s*(?:async\\s+)?def\\s+${ID}`), kind: 'function' },
  { rx: new RegExp(`^\\s*class\\s+${ID}`), kind: 'class' }
];

const GO_DETECTORS: Detector[] = [
  /* Receivers: `func (s *State) Foo(...)` → name in group 2. We
     handle both with one alternation by checking the secondary
     capture when the primary is `func`-keyword-only. */
  { rx: new RegExp(`^func\\s+(?:\\(\\s*[A-Za-z_*\\s]+\\)\\s+)?${ID}`), kind: 'function' },
  { rx: new RegExp(`^type\\s+${ID}\\s+struct\\b`), kind: 'struct' },
  { rx: new RegExp(`^type\\s+${ID}\\s+interface\\b`), kind: 'interface' },
  { rx: new RegExp(`^type\\s+${ID}\\b`), kind: 'type' },
  { rx: new RegExp(`^const\\s+${ID}\\b`), kind: 'variable' },
  { rx: new RegExp(`^var\\s+${ID}\\b`), kind: 'variable' }
];

const SVELTE_DETECTORS: Detector[] = [
  /* Svelte files are HTML-ish with one or more `<script>` blocks; we
     accept TS detectors throughout (a misfire on attribute text is
     vanishingly rare and harmless). */
  ...TS_DETECTORS,
  /* Markdown-style section headers in <style>'s comments — useful in
     long single-file components. */
  { rx: /^\s*\/\*\s*━+\s*(.+?)\s*━+\s*\*\//, kind: 'section' }
];

const MARKDOWN_DETECTORS: Detector[] = [
  /* `## Heading` → level via leading hash count. We capture the
     headline text and emit a single `section` kind regardless of
     level so the picker shows them in document order. */
  { rx: /^(#{1,6})\s+(.+?)\s*#*\s*$/, kind: 'section' }
];

function detectorsFor(path: string): Detector[] {
  const dot = path.lastIndexOf('.');
  if (dot < 0) return TS_DETECTORS;
  const ext = path.slice(dot + 1).toLowerCase();
  switch (ext) {
    case 'ts':
    case 'tsx':
    case 'js':
    case 'jsx':
    case 'mjs':
    case 'cjs':
      return TS_DETECTORS;
    case 'svelte':
      return SVELTE_DETECTORS;
    case 'rs':
      return RUST_DETECTORS;
    case 'py':
    case 'pyi':
      return PY_DETECTORS;
    case 'go':
      return GO_DETECTORS;
    case 'md':
    case 'mdx':
      return MARKDOWN_DETECTORS;
    default:
      return TS_DETECTORS;
  }
}

const MAX_LINES = 8000;

export function extractSymbols(filePath: string, source: string): SymbolEntry[] {
  const detectors = detectorsFor(filePath);
  const lines = source.split('\n');
  const out: SymbolEntry[] = [];
  const limit = Math.min(lines.length, MAX_LINES);
  for (let i = 0; i < limit; i++) {
    const raw = lines[i];
    /* Skip obvious noise — empty / single-line comments — to keep the
       per-line work tight on big files. */
    if (!raw || raw.length > 800) continue;
    let matched: { name: string; kind: SymbolKind } | null = null;
    for (const d of detectors) {
      const m = d.rx.exec(raw);
      if (!m) continue;
      /* Markdown's regex captures level in g1 + name in g2; everything
         else captures name in g1 directly. We sniff group count to
         pick the right group without per-detector branching. */
      const name = m[2] ?? m[1];
      if (!name) continue;
      matched = { name, kind: d.kind };
      break;
    }
    if (!matched) continue;
    const indentMatch = /^(\s*)/.exec(raw);
    const indent = indentMatch ? indentMatch[1].length : 0;
    /* Two-space indent → depth 1, four-space → 2, tab → 1 (one tab
       counts as 2 spaces here so users with 4-space tabs still get a
       sensible nesting). */
    const indentNorm = raw.startsWith('\t')
      ? (raw.match(/^\t+/)?.[0].length ?? 0)
      : Math.floor(indent / 2);
    out.push({
      line: i + 1,
      name: matched.name,
      kind: matched.kind,
      depth: Math.min(indentNorm, 5),
      preview: raw.trim().slice(0, 160)
    });
  }
  return out;
}
