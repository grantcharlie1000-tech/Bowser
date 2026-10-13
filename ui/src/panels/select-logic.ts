/** Pure helpers for the HTML select (`select.ts`): keyboard navigation, typeahead and placement. */

/**
 * Index of the option a typed character or short word moves to. Typing the same letter again
 * cycles through the options that start with it; a longer query matches from the current option.
 * Returns -1 when nothing matches.
 */
export function typeaheadIndex(labels: readonly string[], current: number, query: string): number {
  const needle = query.toLowerCase();
  if (needle === '' || labels.length === 0) return -1;
  const cycling = [...needle].every((char) => char === needle[0]);
  const key = cycling ? needle[0] ?? '' : needle;
  const start = cycling ? current + 1 : Math.max(current, 0);
  for (let step = 0; step < labels.length; step += 1) {
    const index = (start + step) % labels.length;
    if ((labels[index] ?? '').toLowerCase().startsWith(key)) return index;
  }
  return -1;
}

/** Keys that open a closed select. */
export function opensSelect(key: string): boolean {
  return key === 'Enter' || key === ' ' || key === 'ArrowDown' || key === 'ArrowUp' || key === 'Home' || key === 'End';
}

/** True for a printable single character, which feeds the typeahead. */
export function isTypeKey(event: { key: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean }): boolean {
  return event.key.length === 1 && event.key !== ' ' && !event.ctrlKey && !event.metaKey && !event.altKey;
}

export interface Placement { readonly top: number; readonly left: number; readonly width: number; readonly maxHeight: number; readonly above: boolean }

/**
 * Where the list goes relative to its trigger, in viewport pixels. It opens below the trigger and
 * flips above only when it would not fit below and there is more room above. `height` is the
 * list's natural height, `gap` the space kept between trigger and list and `margin` the space kept
 * from the viewport edge.
 */
export function placeList(
  trigger: { top: number; bottom: number; left: number; width: number },
  viewport: { width: number; height: number },
  height: number,
  gap = 4,
  margin = 8,
  minWidth = 0,
): Placement {
  const below = viewport.height - trigger.bottom - gap - margin;
  const above = trigger.top - gap - margin;
  const flip = height > below && above > below;
  const room = Math.max(flip ? above : below, 48);
  const shown = Math.min(height, room);
  const width = Math.min(Math.max(trigger.width, minWidth), Math.max(viewport.width - margin * 2, 0));
  const left = Math.min(Math.max(trigger.left, margin), Math.max(viewport.width - margin - width, margin));
  const top = flip ? trigger.top - gap - shown : trigger.bottom + gap;
  return { top, left, width, maxHeight: room, above: flip };
}
