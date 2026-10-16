// Pure parts of the Tabs settings in the strip (Settings > Tabs): the marks a tab carries, how a click
// changes the selection, the wheel, and the width lock. No DOM and no runtime imports, so
// test/tab-options.mjs loads it directly.

/** How a loading tab shows it: the spinner in place of the favicon, a line along the tab, a ring on the favicon, or nothing. */
export type LoadMark = 'spin' | 'bar' | 'ring' | 'none';
export type LoadIndicatorChoice = 'spinner' | 'bar' | 'favicon' | 'none';

/**
 * What to draw for a tab's page load under the chosen indicator. A tab with no progress figure yet
 * falls back to the spinner, so a load is never invisible.
 */
export function loadMark(indicator: LoadIndicatorChoice, loading: boolean, pct: number | undefined): LoadMark {
  if (!loading || indicator === 'none') return 'none';
  if (indicator === 'spinner' || pct === undefined) return 'spin';
  return indicator === 'bar' ? 'bar' : 'ring';
}

/** The count on a tab: 1 to 99, then "99+". */
export function notifyLabel(count: number): string {
  return count > 99 ? '99+' : String(count);
}

/** The words next to a periodic reload: "45s", "5m", "1h". Rounds up, so the last second reads 1s, not 0s. */
export function reloadLabel(secs: number): string {
  if (secs >= 3600) return `${Math.ceil(secs / 3600)}h`;
  if (secs >= 60) return `${Math.ceil(secs / 60)}m`;
  return `${Math.max(0, Math.ceil(secs))}s`;
}

/** The reload countdown after `elapsedMs` of local ticking from `startSecs`. Never negative. */
export function reloadRemaining(startSecs: number, elapsedMs: number): number {
  return Math.max(0, Math.ceil(startSecs - elapsedMs / 1000));
}

export interface SelectionClick { readonly ctrl: boolean; readonly shift: boolean }
export interface SelectionResult {
  /** Tab ids selected afterwards, in strip order. */
  readonly selected: readonly number[];
  /** The tab the next Shift click ranges from. */
  readonly anchor: number;
  /** Whether the click also makes the tab the active one (a plain click does; Ctrl and Shift clicks only select). */
  readonly activate: boolean;
}

/**
 * Applies a click on tab `id` to the selection. `order` is the visible tabs in strip order and `active`
 * the active tab. A plain click clears the selection. Ctrl toggles one tab; the active tab joins the
 * selection first, so Ctrl+click on a second tab selects two. Shift selects the range from the anchor
 * (the active tab when none was clicked yet) to the tab, both ends included.
 */
export function clickSelection(
  selected: readonly number[],
  order: readonly number[],
  anchor: number,
  active: number,
  id: number,
  click: SelectionClick,
): SelectionResult {
  if (!click.ctrl && !click.shift) return { selected: [], anchor: id, activate: true };
  if (click.shift) {
    const from = order.indexOf(order.includes(anchor) ? anchor : active);
    const to = order.indexOf(id);
    if (from < 0 || to < 0) return { selected, anchor, activate: false };
    const [lo, hi] = from <= to ? [from, to] : [to, from];
    return { selected: order.slice(lo, hi + 1), anchor: order[from] ?? active, activate: false };
  }
  const next = new Set(selected.length === 0 && order.includes(active) ? [active] : selected);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  return { selected: order.filter((tab) => next.has(tab)), anchor: id, activate: false };
}

/** Drops ids that are no longer tabs (a closed tab leaves the selection). */
export function pruneSelection(selected: readonly number[], live: ReadonlySet<number>): number[] {
  return selected.filter((id) => live.has(id));
}

/** Wheel pixels for one tab step: a wheel notch sends 100, a trackpad many small deltas. */
export const WHEEL_STEP_PX = 60;

/** Adds a wheel delta to the running total; returns the whole steps it makes and what is left over. */
export function wheelSteps(total: number, delta: number): { steps: number; rest: number } {
  const sum = total + delta;
  const steps = Math.trunc(sum / WHEEL_STEP_PX);
  return { steps, rest: sum - steps * WHEEL_STEP_PX };
}

/** The tab `steps` away from `active` among `order`, stopping at the ends. Null when that is the same tab. */
export function stepTab(order: readonly number[], active: number, steps: number): number | null {
  const at = order.indexOf(active);
  if (at < 0 || order.length === 0 || steps === 0) return null;
  const to = Math.min(order.length - 1, Math.max(0, at + steps));
  return to === at ? null : order[to] ?? null;
}

/** The width every tab holds after a close with the pointer on the strip, so the next close button lands under it. */
export function lockedWidth(closedWidth: number, floor = 32): number {
  return Number.isFinite(closedWidth) ? Math.max(floor, Math.round(closedWidth)) : floor;
}

/** The strip classes the options turn on, in a fixed order. */
export function stripClasses(opts: {
  readonly closeLeft: boolean; readonly closeAlways: boolean; readonly scroll: boolean;
  readonly separator: boolean; readonly removeSpacing: boolean; readonly maximised: boolean;
}): string[] {
  const out: string[] = [];
  if (opts.closeLeft) out.push('close-left');
  if (opts.closeAlways) out.push('close-always');
  if (opts.scroll) out.push('scrolling');
  if (opts.separator) out.push('pin-sep');
  if (opts.removeSpacing && opts.maximised) out.push('flush');
  return out;
}

/** The accessible name of a tab with marks: "Inbox, 3 notifications, unread". */
export function markedName(title: string, notify: number | undefined, unread: boolean | undefined, reload: number | undefined): string {
  const parts = [title];
  if (notify !== undefined && notify > 0) parts.push(notify === 1 ? '1 notification' : `${notify} notifications`);
  if (unread === true) parts.push('unread');
  if (reload !== undefined) parts.push(`reloads in ${reloadLabel(reload)}`);
  return parts.join(', ');
}
