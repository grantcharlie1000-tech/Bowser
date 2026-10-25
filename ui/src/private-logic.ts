// Private tab landing (docs/design/private-tabs.md): the title it shows and the parts of the private
// mark it animates. Pure and DOM-free, so test/private-page.mjs can check the mark against the
// generated icon data without a page.
//
// The landing shows only this title. What a private tab may persist is unchanged (src/private.rs,
// src/app.rs, src/engine/webview2.rs) and is spelled out on the Privacy dashboard, not here.

export const PRIVATE_PAGE_TITLE = 'You’re browsing privately';

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
