// Curated pool of evocative names used as identifiers for multi-instance
// apps (Editor, Canvas, Terminal). When a user spins up a second editor
// or terminal, we pick the next unused name from this list and use it as
// both the human-readable label ("Editor · Vermeer") and a slug-safe
// suffix on the underlying instance id ("editor:vermeer").
//
// The names mix Renaissance / Modern painters, composers, evocative
// places, and works of art — all single-word, single-syllable-ish, and
// internationally recognisable. Single-word so the rail tooltip stays
// short; recognisable so they stick in memory ("the Vermeer terminal").

const POOL: readonly string[] = [
  // Painters
  'Vermeer', 'Rothko', 'Hopper', 'Hokusai', 'Klimt', 'Magritte', 'Hilma',
  'Frida', 'Basquiat', 'Lempicka', 'Goya', 'Caravaggio', 'Bosch', 'Klee',
  'Mondrian', 'Pollock', 'Kahlo', 'Sargent', 'Turner', 'Cezanne',
  // Composers
  'Glass', 'Reich', 'Pärt', 'Sibelius', 'Ravel', 'Satie', 'Debussy',
  'Chopin', 'Bartók', 'Mahler', 'Brahms', 'Schubert', 'Schumann',
  'Stravinsky', 'Prokofiev', 'Shostakovich',
  // Places
  'Kyoto', 'Lisbon', 'Reykjavik', 'Marrakesh', 'Florence', 'Bruges',
  'Petra', 'Chefchaouen', 'Sintra', 'Hoi-An', 'Kotor', 'Hampi',
  // Works
  'Mona-Lisa', 'Starry-Night', 'Sunflowers', 'Wave', 'Persistence',
  'Scream', 'Pearl', 'Birth', 'Nighthawks', 'Guernica'
];

/** Pick the first name from the pool that isn't already used.
 *  Falls back to a numeric suffix if (somehow) every name is taken. */
export function pickInstanceName(used: ReadonlyArray<string>): string {
  const taken = new Set(used.map((n) => n.toLowerCase()));
  for (const n of POOL) {
    if (!taken.has(n.toLowerCase())) return n;
  }
  // Pool exhausted (60+ instances of one app — extreme edge case).
  // Append `-N` to the last name so we don't loop forever.
  let i = 2;
  while (taken.has((POOL[POOL.length - 1] + '-' + i).toLowerCase())) i++;
  return POOL[POOL.length - 1] + '-' + i;
}

/** Convert a display name to a slug-safe id chunk. */
export function nameToSlug(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
}
