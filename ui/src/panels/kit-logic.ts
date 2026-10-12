/** Pure helpers for the shared panel kit (keyboard navigation, text trimming, drop zones). */

/** Index reached from `current` by a list navigation key, clamped to the list. -1 means none yet. */
export function navIndex(count: number, current: number, key: string): number {
  if (count <= 0) return -1;
  if (key === 'Home') return 0;
  if (key === 'End') return count - 1;
  if (key !== 'ArrowDown' && key !== 'ArrowUp') return current;
  if (current < 0) return key === 'ArrowDown' ? 0 : count - 1;
  return Math.max(0, Math.min(count - 1, current + (key === 'ArrowDown' ? 1 : -1)));
}

/** True for the keys the roving list handler owns. */
export function isNavKey(key: string): boolean {
  return key === 'ArrowDown' || key === 'ArrowUp' || key === 'Home' || key === 'End';
}

/** One line preview of a note body: whitespace collapsed, cut at `max` characters. */
export function preview(text: string, max = 80): string {
  const flat = text.replace(/\s+/g, ' ').trim();
  return flat.length <= max ? flat : `${flat.slice(0, max - 1).trimEnd()}…`;
}

/** First letter for a placeholder favicon tile. */
export function tileLetter(text: string): string {
  const match = /[\p{L}\p{N}]/u.exec(text);
  return match ? match[0].toUpperCase() : '·';
}

/** Where a dragged row lands relative to the row under the pointer. */
export type DropZone = 'before' | 'into' | 'after';
export function dropZone(offsetY: number, height: number, isFolder: boolean): DropZone {
  if (height <= 0) return 'after';
  const ratio = offsetY / height;
  if (isFolder) return ratio < 0.28 ? 'before' : ratio > 0.72 ? 'after' : 'into';
  return ratio < 0.5 ? 'before' : 'after';
}
