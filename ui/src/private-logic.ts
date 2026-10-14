// Private tab landing (docs/design/private-tabs.md): the words it shows and the parts of the private
// mark it animates. Pure and DOM-free, so test/private-page.mjs can check every claim against the
// rules in src/private.rs and the engine without a page.
//
// Every sentence here must stay true of the code: src/private.rs (what a private tab may persist),
// src/app.rs (no recently-closed entry, no restore, no group), src/engine/webview2.rs (InPrivate
// profile discarded with its last controller, every permission denied, downloads cancelled, strict
// tracking prevention, the always-on blocker). Change the code and these lines together.
import type { IconName } from './icons';

export interface PrivateRow {
  readonly icon: IconName;
  readonly label: string;
  readonly text: string;
}

export const PRIVATE_PAGE_EYEBROW = 'Private tab';
export const PRIVATE_PAGE_TITLE = 'You’re browsing privately';
export const PRIVATE_PAGE_LEAD =
  'Bowser doesn’t record your browsing in private tabs. Their cookies and site data stay in memory and are erased when you close your last private tab.';

export const PRIVATE_KEPT_TITLE = 'Bowser won’t keep';
export const PRIVATE_NOT_KEPT: readonly PrivateRow[] = [
  { icon: 'clock', label: 'Pages you visit', text: 'Not added to History, address bar suggestions or Recently closed.' },
  { icon: 'cookie', label: 'Cookies and site data', text: 'Held in memory only, and erased with your last private tab.' },
  { icon: 'lock', label: 'Permissions', text: 'Camera, location, notifications and every other request are blocked, and no choice is saved.' },
  { icon: 'reset', label: 'Your private tabs', text: 'Never restored after a restart or crash, grouped, or saved in a session or workspace.' },
  { icon: 'download', label: 'Downloads', text: 'Blocked, so no file is left on this PC.' },
];

export const PRIVATE_SEEN_TITLE = 'Still visible to';
export const PRIVATE_STILL_SEEN: readonly PrivateRow[] = [
  { icon: 'workspace-globe', label: 'Websites you visit', text: 'They see your IP address, and know you when you sign in.' },
  { icon: 'eye', label: 'Your network', text: 'Your internet provider, employer or school can see which sites you reach.' },
  { icon: 'bookmark', label: 'Bookmarks you add', text: 'Saved like any other bookmark, because you chose to keep them.' },
];

export const PRIVATE_SHARED_NOTE =
  'Private tabs share one session: sign in to a site in one and it knows you in the others, until your last private tab closes. Ads and trackers are always blocked here, with strict tracking prevention.';

export const PRIVATE_CLOSE_ALL = 'Close all private tabs';

/** "1 private tab open" / "3 private tabs open". */
export function privateCountText(n: number): string {
  const count = Math.max(1, Math.floor(n));
  return `${count} private ${count === 1 ? 'tab' : 'tabs'} open`;
}

/**
 * The shapes of icon('private') (icons/extra/private.svg, in source order) that the landing groups
 * for its animation: the bowler (brim, crown) and the shades (left lens, right lens, bridge). The
 * lenses are the filled shapes the glint is clipped to. test/private-page.mjs checks this against
 * the generated icon data, so a redrawn mark cannot silently break the animation.
 */
export const MARK_PARTS = {
  hat: [0, 1],
  shades: [2, 3, 4],
  lenses: [2, 3],
} as const;
export const MARK_SHAPE_COUNT = 5;
