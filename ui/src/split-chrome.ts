// Split view marks on the other chrome views: the joined pair in the tab strip and the vertical
// sidebar, and the toolbar's split button. Styles live in styles/split.css.
import type { PairMsg } from './split-protocol';
import { livePair, paneDescription, paneFocused, pairRole, splitGlyph } from './split-logic';
import { icon } from './dom';
import { setTip } from './tooltip';

/**
 * Marks the layout's tab elements (`.tab` in the strip, `.side-tab` in the sidebar) under `root`:
 * `data-split` = `l` (first), `m` (a middle pane of three or four) or `r` (last) for the joined
 * capsule, `data-pair` = `shown` / `parked`, `data-pair-focus` on the focused pane of a shown
 * layout, and an accessible description. Every other tab loses the marks.
 */
export function markPair(root: HTMLElement, msg: PairMsg | null): void {
  const pair = livePair(msg);
  for (const node of Array.from(root.querySelectorAll<HTMLElement>('.tab[data-id], .side-tab[data-id]'))) {
    const role = pairRole(Number(node.dataset.id), pair);
    if (role === null || pair === null) {
      if (node.dataset.split !== undefined) {
        delete node.dataset.split;
        delete node.dataset.pair;
        delete node.dataset.pairFocus;
        node.removeAttribute('aria-description');
      }
      continue;
    }
    if (node.dataset.split !== role) node.dataset.split = role;
    const state = pair.shown ? 'shown' : 'parked';
    if (node.dataset.pair !== state) node.dataset.pair = state;
    const id = Number(node.dataset.id);
    if (paneFocused(id, pair)) node.dataset.pairFocus = 'true';
    else delete node.dataset.pairFocus;
    const description = paneDescription(id, pair);
    if (node.getAttribute('aria-description') !== description) node.setAttribute('aria-description', description);
  }
}

/**
 * The toolbar's split button: pressed while a split is shown, with the focused pane's place filled
 * in its glyph (`split-left` / `split-right`, `split-top` / `split-bottom` when stacked, or a
 * quarter such as `split-top-left` in a layout of three or four panes), as Chrome's split icon does.
 */
export function syncSplitButton(button: HTMLButtonElement, msg: PairMsg | null): void {
  const pair = livePair(msg);
  const shown = pair !== null && pair.shown;
  const glyph = splitGlyph(pair);
  if (button.dataset.glyph !== glyph) {
    button.dataset.glyph = glyph;
    button.replaceChildren(icon(glyph));
  }
  button.setAttribute('aria-pressed', String(shown));
  const label = shown ? 'Split view options' : 'Open split view';
  if (button.getAttribute('aria-label') !== label) {
    button.setAttribute('aria-label', label);
    setTip(button, shown ? 'Split view options' : 'Split view (Ctrl+Shift+\\)');
  }
}
