// Pure tab "lift" logic shared by the strip (tabs.ts) and the vertical sidebar (sidebar.ts): no DOM,
// no runtime imports. See docs/design/tab-drag.md.

/** Pointer travel (CSS px, either axis) before a press becomes a drag. */
export const DRAG_START_PX = 4;
/**
 * How far (CSS px) the pointer may leave the strip's band before the tab detaches and floats.
 * Chromium's kVerticalDetachMagnetism is 15 DIP; ours is a little larger because the toolbar row
 * sits between the strip and the page, and it must stay above split-logic's DRAG_OUT_PX (12) so a
 * floating tab is always over the page as far as the split drop zones are concerned.
 */
export const DETACH_PX = 24;
/** A floating tab re-attaches once the pointer is back within this distance of the band (hysteresis). */
export const REATTACH_PX = 12;
/** The ghost's glide to its slot (ms). Rust's `GLIDE_MS` is the same value. */
export const GHOST_SETTLE_MS = 200;
/** Longest the page keeps its own tab hidden after a release before showing it anyway (ms). */
export const GHOST_REVEAL_FALLBACK_MS = 600;
/** Keepalive cadence while a ghost is up; Rust hides a ghost that hears nothing for 2 s. */
export const GHOST_KEEPALIVE_MS = 500;

export type DragPhase = 'attached' | 'floating';

/** The region a held tab is attached to, in client px. Open sides are +/-Infinity. */
export interface Band {
  readonly left: number;
  readonly top: number;
  readonly right: number;
  readonly bottom: number;
}

export interface Rectish {
  readonly left: number;
  readonly top: number;
  readonly right: number;
  readonly bottom: number;
}

/** The horizontal strip: only its top and bottom edges detach (beyond its ends is outside the window). */
export function stripBand(strip: Rectish): Band {
  return { left: -Infinity, top: strip.top, right: Infinity, bottom: strip.bottom };
}

/** The vertical sidebar: only its right edge (towards the page) detaches; up and down auto-scroll. */
export function sidebarBand(sidebar: Rectish): Band {
  return { left: -Infinity, top: -Infinity, right: sidebar.right, bottom: Infinity };
}

/** How far (px) the point is outside the band; 0 inside. */
export function bandDistance(band: Band, x: number, y: number): number {
  const dx = Math.max(band.left - x, x - band.right, 0);
  const dy = Math.max(band.top - y, y - band.bottom, 0);
  return Math.max(dx, dy);
}

/**
 * Attached <-> floating with hysteresis: an attached tab floats once the pointer is more than
 * `detach` px outside the band; a floating one re-attaches within `reattach` px.
 */
export function nextPhase(prev: DragPhase, band: Band, x: number, y: number, detach = DETACH_PX, reattach = REATTACH_PX): DragPhase {
  const d = bandDistance(band, x, y);
  if (prev === 'attached') return d > detach ? 'floating' : 'attached';
  return d <= reattach ? 'attached' : 'floating';
}

/** Where the press landed inside the tab (CSS px, whole, clamped to the tab): it stays under the cursor. */
export function grabOffset(pointerX: number, pointerY: number, rect: { left: number; top: number; width: number; height: number }): { gx: number; gy: number } {
  const w = Math.max(1, Math.round(rect.width));
  const h = Math.max(1, Math.round(rect.height));
  const clamp = (v: number, hi: number): number => Math.min(hi, Math.max(0, Math.round(Number.isFinite(v) ? v : 0)));
  return { gx: clamp(pointerX - rect.left, w), gy: clamp(pointerY - rect.top, h) };
}

/** Top-left of the held tab for a pointer at (x, y) with grab offset (gx, gy): x - gx, y - gy. */
export function ghostOrigin(x: number, y: number, grab: { gx: number; gy: number }): { x: number; y: number } {
  return { x: x - grab.gx, y: y - grab.gy };
}

/**
 * Horizontal offsets that open the held tab's gap at `dest` (attached) or close it (floating), for
 * tabs at `lefts` with `widths` and `gap` px between neighbours. Index `from` is the held tab: its
 * entry is where its (hidden) slot sits, so a release can glide the ghost there.
 * Exact for a contiguous run of tabs: a tab passed over moves by the held tab's width plus one gap.
 */
export function gapShifts(lefts: readonly number[], widths: readonly number[], gap: number, from: number, dest: number, floating: boolean): number[] {
  const n = lefts.length;
  const out = new Array<number>(n).fill(0);
  if (from < 0 || from >= n) return out;
  const step = (widths[from] ?? 0) + gap;
  if (floating) {
    for (let i = from + 1; i < n; i++) out[i] = -step;
    return out;
  }
  const to = Math.max(0, Math.min(n - 1, dest));
  if (to > from) {
    for (let i = from + 1; i <= to; i++) out[i] = -step;
    const right = (lefts[to] ?? 0) + (widths[to] ?? 0);
    out[from] = right - ((lefts[from] ?? 0) + (widths[from] ?? 0));
  } else if (to < from) {
    for (let i = to; i < from; i++) out[i] = step;
    out[from] = (lefts[to] ?? 0) - (lefts[from] ?? 0);
  }
  return out;
}

export type DropOutcome = 'reorder' | 'settle' | 'split' | 'cancel';

/**
 * What a release does: attached = reorder (or settle back into the same slot); floating over the
 * page = split drop (Rust decides whether a zone was showing); floating anywhere else, Escape, or a
 * cancelled pointer = cancel (the ghost glides home).
 */
export function dropOutcome(commit: boolean, phase: DragPhase, overPage: boolean, from: number, dest: number): DropOutcome {
  if (!commit) return 'cancel';
  if (phase === 'attached') return dest !== from ? 'reorder' : 'settle';
  return overPage ? 'split' : 'cancel';
}

/** A settle target on the wire: whole CSS px, clamped to the protocol range. */
export function settlePoint(x: number, y: number): { x: number; y: number } {
  const clamp = (v: number): number => Math.min(16384, Math.max(0, Math.round(Number.isFinite(v) ? v : 0)));
  return { x: clamp(x), y: clamp(y) };
}

/** Screen-reader text for each moment of a drag. */
export function dragAnnouncement(kind: 'lift' | 'float' | 'attach' | 'drop' | 'cancel' | 'split', title: string, position = 0, count = 0): string {
  const name = title !== '' ? title : 'Tab';
  switch (kind) {
    case 'lift': return `Picked up ${name}. Move to reorder, drag onto the page to split, Escape to cancel.`;
    case 'float': return `${name} is off the tab strip. Release over the page to open it in split view.`;
    case 'attach': return `${name} is back on the tab strip.`;
    case 'drop': return count > 0 ? `${name} moved to position ${position} of ${count}.` : `${name} dropped.`;
    case 'split': return `${name} dropped on the page.`;
    case 'cancel': return `Move cancelled. ${name} returned to its place.`;
  }
}
