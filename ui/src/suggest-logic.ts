// --- omnibox suggest ---
// Pure rules for the omnibox dropdown, shared by the toolbar (keyboard) and the hover view (rows).
import type { IconName } from './dom';
import type { SuggestKind } from './protocol';

/** Row after `sel` when the arrow `key` is pressed in a list of `count` rows; wraps at both ends. */
export function moveSuggestSelection(sel: number, count: number, key: 'ArrowDown' | 'ArrowUp'): number {
  if (count <= 0) return 0;
  const step = key === 'ArrowDown' ? 1 : count - 1;
  return (Math.max(0, Math.min(count - 1, sel)) + step) % count;
}

/** Text the omnibox shows for row `sel`: the user's own text for the default row, else the row's. */
export function suggestPreviewText(typed: string, sel: number, rowText: string): string {
  return sel === 0 ? typed : rowText;
}

/** Icon shown in a row that has no favicon. */
export function suggestGlyph(kind: SuggestKind): IconName {
  switch (kind) {
    case 'search': return 'search';
    case 'hist': return 'history';
    case 'bm': return 'bookmark';
    case 'tab': return 'new-tab';
    default: return 'link';
  }
}
// --- end omnibox suggest ---
