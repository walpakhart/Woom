// Persist-error reactive state — split out of sessions.svelte.ts in
// wave-1 phase-7 refactor so the disk-write module
// (`sessions_disk.svelte.ts`) and the rules-write code path in the
// host can share the same reactive shape without a circular import.
// Settings reads it directly via `persistError.sessions` /
// `persistError.rules` to drive the "Storage" badge + blocker banner.
// `null` on either slot = healthy; a string = last seen error
// message from the failing write.

export const persistError = $state<{
  sessions: string | null;
  rules: string | null;
}>({
  sessions: null,
  rules: null,
});
