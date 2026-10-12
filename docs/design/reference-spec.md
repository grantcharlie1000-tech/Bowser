# Bowser reference spec

Sources: Microsoft Learn (Geometry, Layering, Motion, Typography in Windows 11) for all Fluent numbers. Images in `references/` come from Wikimedia Commons screenshots, so check each file's licence before reuse. Per-browser pixel values are measured or estimated from those screenshots and marked ~.

## Surface patterns

**Tab strip (horizontal).** Edge, Chrome and Vivaldi (`edge-win11`, `chrome-win11`, `vivaldi-welcome`) use a 36-40px strip. Bowser should use a 36px strip with 28px-tall islands, 8px radius (4px on dense lists), and 4px gaps. Active island is 1.0 glass fill plus a 1px hairline. Inactive tabs have no fill until hover. The 16px favicon sits 8px from the edge, and the close button is a 20px box with a 16px glyph, shown on hover or when active. Groups use a 2px coloured underline and a 6px dot; do not add pill chrome around them.

**Vertical sidebar.** Arc, Zen and Edge (`arc-windows-1.3.2`, `zen-win11-dark`, `zen-mac-1.8.2`). Use a 240px expanded and 48px collapsed rail. Rows are 32px tall with an 8px side inset and 6px radius. The active row is a raised island, not a bar. Put the new-tab row first, workspaces at the bottom as 20px icon buttons, and pinned tiles as a 3-up grid of 32px squares. Collapse takes 167ms.

**Address bar and suggestions.** The pill is centred (Arc, `arc-windows-1.3.2`): 32px tall, ~560px max, full radius, with the site-info lock at the left and actions at the right. Edge's bar (`edge-win11`) is full width, so keep Bowser centred. Suggestions drop from the pill as one 8px overlay, with rows 36px tall, 12px gutters, and a 16px leading icon. The top corners attach to the pill (Fluent AutoSuggest rule: square the corners that touch the invoker). Max 8 rows, highlighted row 4px radius.

**Site info popup.** 320px wide, 8px radius, header row 44px, then 36px permission rows with a 16px icon and a toggle. The lock icon is the anchor, and the arrow is omitted.

**Context menus.** Edge-style but flat (`edge-109`). 8px overlay radius, 4px padding, items 32px tall with 4px radius, 12px icon-text gap, and a 1px separator with 4px margin. Accelerators are right-aligned in Caption at 60% alpha. Min width 200px.

**Settings page.** Left nav of 220px (32px rows), single 640px content column. Section titles are BodyStrong with no uppercase eyebrows. Rows are 48px tall: label and description on the left, control on the right, hairline separators. No cards, to avoid the SaaS look (`firefox-136-mac`, `edge-win11`).

**Downloads and history panels.** Anchored 360px flyout (Chrome/Edge). 56px rows with a 32px file icon, name in Body, and status in Caption. A progress bar is 4px tall with 4px radius. The history page is a dense 32px list grouped by sentence-case day headers.

**Permission prompts.** Anchored under the lock, 320px, 8px radius, 16px padding. Title in BodyStrong, 12px gap, then two buttons (28px tall, 4px radius) right-aligned. The primary is accent-filled. There is a single prompt at a time, with no modal blocking.

**Tooltips.** Caption 12/16, 4px radius, 6x8px padding, 500ms hover delay, 0ms when moving between neighbours, elevation 16.

**Command palette (Ctrl+K).** The pattern is Arc/Vivaldi quick-command, 560px wide, anchored 12% from the top, 44px input, 36px results, one overlay. Opens in 83ms fade plus 8px rise.

**Split view.** Arc and Vivaldi use two panes with a 4px gutter and an 8px page radius. Show a 2px accent edge on the focused pane. The divider is draggable at 8px hit width.

**Icons.** 16px glyph in a 20px box, 1.5px stroke on a 24 grid, round caps, matching `icons/menu` and `icons/rail`. Use a single family only.

## Fluent facts used

- Radii: 8px for overlays (flyout, dialog, menu, teaching tip), 4px for controls and tooltips, and 0px when maximised or snapped.
- Elevation (value, 1px contour): window and dialog 128, flyout 32, tooltip 16, card 8, control 2, layer 1.
- Type: Caption 12/16, Body 14/20, BodyStrong 14/20 semibold (600), Subtitle 20/28. Segoe UI Variable has an automatic optical axis. Use sentence case and no italics.
- Motion: direct entrance cubic-bezier(0,0,0,1) at 167/250/333ms. Point-to-point cubic-bezier(0.55,0.55,0,1). Direct exit cubic-bezier(0,0,0,1) at 167ms, always combined with a fade. Fade is linear at 83ms.
- Focus ring: Fluent 2 uses a 2px outer ring with a 1px inner ring in the opposite colour (from memory, not fetched). Bowser adopts 2px accent plus 1px contrast inner offset.

## Proposed tokens

Prefixed `--bw-`. Glass fills are alpha overlays on the window material.

| Token | Light | Dark |
|---|---|---|
| surface-glass | rgba(255,255,255,.55) | rgba(32,32,32,.60) |
| island | rgba(255,255,255,.85) | rgba(255,255,255,.10) |
| island-hover | rgba(255,255,255,.55) | rgba(255,255,255,.06) |
| overlay (menus, popups) | rgba(252,252,252,.94) | rgba(44,44,44,.94) |
| text | #1A1A1A | #FFFFFF |
| text-2 | rgba(0,0,0,.62) | rgba(255,255,255,.70) |
| hairline | rgba(0,0,0,.08) | rgba(255,255,255,.10) |
| accent | #0067C0 | #4CC2FF |
| focus | accent 2px + 1px inner contrast | same |

| Group | Tokens |
|---|---|
| Radius | `r-control` 4, `r-island` 8, `r-overlay` 8, `r-pill` 999 |
| Spacing | 2, 4, 6, 8, 12, 16, 24 (4px base, no 5 or 10) |
| Shadow | `sh-tip` 0 2px 4px rgba(0,0,0,.14); `sh-flyout` 0 8px 16px rgba(0,0,0,.14), 0 0 2px rgba(0,0,0,.12); `sh-dialog` 0 32px 64px rgba(0,0,0,.24) |
| Duration | `fast` 83ms (opacity), `base` 167ms (position, scale), `slow` 250ms (sidebar width) |
| Easing | `out` cubic-bezier(0,0,0,1); `move` cubic-bezier(.55,.55,0,1) |
| Z-layers | page 0, chrome 10, sidebar-flyout 20, menu 30, popup 40, tooltip 50, palette 60, dialog 70 |

Reduced motion: all durations 0ms except 83ms opacity.

Also in `references/`: `arc-mac`, `zen-win11-light`, `vivaldi-macos`, `safari-18.4` (compact bar), `firefox-136-mac` (restrained toolbar). Not obtained: Orion, SigmaOS, Firefox vertical tabs.
