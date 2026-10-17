// Rail glyphs, drawn from the shared icon family (icons.ts). The wire names (rail-logic.ts, sent
// by Rust) map onto family names here; the SVG sources are icons/rail/*.svg, which
// src/chrome/rail_icons.rs also ports for the native rail.
import { icon, type IconName } from './icons';
import type { RailIconName } from './rail-logic';

/** Family icon for each wire name. `letter` is a text tile built by rail.ts, never an icon. */
export const RAIL_ICON: Readonly<Record<Exclude<RailIconName, 'letter'>, IconName>> = {
  home: 'home',
  customize: 'customize',
  plus: 'plus',
  heart: 'heart',
  clock: 'clock',
  dots: 'more-vertical',
  feed: 'feed',
  apptile: 'grid', // a pinned app without a favicon; the grid+plus family glyph 'app-tile' is the add-app button
  'g:briefcase': 'workspace-briefcase',
  'g:home': 'workspace-home',
  'g:code': 'workspace-code',
  'g:book': 'workspace-book',
  'g:cart': 'workspace-cart',
  'g:game': 'workspace-game',
  'g:music': 'workspace-music',
  'g:heart': 'workspace-heart',
  'g:star': 'workspace-star',
  'g:bolt': 'workspace-bolt',
  'g:globe': 'workspace-globe',
  'g:leaf': 'workspace-leaf',
};

/** An inline SVG for `name` (an empty one for `letter`, which rail.ts renders as text). */
export function railIcon(name: RailIconName): SVGSVGElement {
  if (name !== 'letter') return icon(RAIL_ICON[name]);
  const empty = icon('plus');
  empty.replaceChildren();
  return empty;
}
