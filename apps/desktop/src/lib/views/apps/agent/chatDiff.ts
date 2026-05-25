// Tiny LCS-based line diff for the inline edit-card preview inside
// ChatThread. Extracted in wave-1 phase-6 refactor. Pure — no Svelte
// state, no DOM. Capped at 400 lines per side so a giant write
// doesn't freeze the UI (the DP table is O(m·n)).

export type DiffRow = {
  kind: 'add' | 'rem' | 'ctx';
  oldNo?: number;
  newNo?: number;
  text: string;
};

const LINE_CAP = 400;

/** LCS line diff. Good enough for chat-card preview — not competing
 *  with `diff`. Returns ordered rows tagged add / rem / ctx; long
 *  stretches of context are collapsed with a "··· N unchanged" hint
 *  (2 lines of locality padding around each change). */
export function computeDiffRows(oldText: string, newText: string): DiffRow[] {
  const a = oldText.split('\n');
  const b = newText.split('\n');
  const aTrim = a.length > LINE_CAP ? a.slice(0, LINE_CAP) : a;
  const bTrim = b.length > LINE_CAP ? b.slice(0, LINE_CAP) : b;
  const m = aTrim.length, n = bTrim.length;
  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0));
  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      if (aTrim[i - 1] === bTrim[j - 1]) dp[i][j] = dp[i - 1][j - 1] + 1;
      else dp[i][j] = Math.max(dp[i - 1][j], dp[i][j - 1]);
    }
  }
  const rows: DiffRow[] = [];
  let i = m, j = n;
  while (i > 0 && j > 0) {
    if (aTrim[i - 1] === bTrim[j - 1]) {
      rows.push({ kind: 'ctx', oldNo: i, newNo: j, text: aTrim[i - 1] });
      i--; j--;
    } else if (dp[i - 1][j] >= dp[i][j - 1]) {
      rows.push({ kind: 'rem', oldNo: i, text: aTrim[i - 1] });
      i--;
    } else {
      rows.push({ kind: 'add', newNo: j, text: bTrim[j - 1] });
      j--;
    }
  }
  while (i > 0) { rows.push({ kind: 'rem', oldNo: i, text: aTrim[i - 1] }); i--; }
  while (j > 0) { rows.push({ kind: 'add', newNo: j, text: bTrim[j - 1] }); j--; }
  rows.reverse();
  return collapseContext(rows, 2);
}

function collapseContext(rows: DiffRow[], pad: number): DiffRow[] {
  const out: DiffRow[] = [];
  const n = rows.length;
  for (let i = 0; i < n; i++) {
    const r = rows[i];
    if (r.kind !== 'ctx') { out.push(r); continue; }
    let next = i;
    while (next < n && rows[next].kind === 'ctx') next++;
    const runLen = next - i;
    const isHead = out.length === 0;
    const isTail = next >= n;
    const head = isHead ? 0 : pad;
    const tail = isTail ? 0 : pad;
    if (runLen <= head + tail + 1) {
      for (let k = i; k < next; k++) out.push(rows[k]);
    } else {
      for (let k = i; k < i + head; k++) out.push(rows[k]);
      out.push({ kind: 'ctx', text: `··· ${runLen - head - tail} unchanged lines ···` });
      for (let k = next - tail; k < next; k++) out.push(rows[k]);
    }
    i = next - 1;
  }
  return out;
}

/** Sum add/rem counts for the edit-card header stats chip. */
export function diffStats(oldText: string, newText: string): { add: number; rem: number } {
  const rows = computeDiffRows(oldText ?? '', newText ?? '');
  let add = 0, rem = 0;
  for (const r of rows) {
    if (r.kind === 'add') add++;
    else if (r.kind === 'rem') rem++;
  }
  return { add, rem };
}
