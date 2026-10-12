/** Moves among stable history row ids, skipping day headings. */
export function moveHistorySelection(ids: readonly number[], selected: number | null, key: string): number | null {
  if (ids.length === 0 || (key !== 'ArrowUp' && key !== 'ArrowDown')) return selected;
  const current = selected === null ? -1 : ids.indexOf(selected);
  const next = current < 0 ? (key === 'ArrowDown' ? 0 : ids.length - 1) :
    Math.max(0, Math.min(ids.length - 1, current + (key === 'ArrowDown' ? 1 : -1)));
  return ids[next] ?? null;
}

/** A panel child that already handled Escape gets first chance to clear its own inline state. */
export function shouldDismissPanelEscape(defaultPrevented: boolean): boolean {
  return !defaultPrevented;
}

/** Builds an open command only for ids that fit the bridge's unsigned 64 bit range. */
export function historyOpenCommand(id: number, newTab: boolean): { v: 1; t: 'hist'; a: 'open' | 'newtab'; id: number } | null {
  if (!Number.isSafeInteger(id) || id <= 0 || id > 0xffffffff) return null;
  return { v: 1, t: 'hist', a: newTab ? 'newtab' : 'open', id };
}
