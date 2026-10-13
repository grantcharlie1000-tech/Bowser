# Visual gate: Bowser HTML chrome (2026-10-13)

Build under test: `target-director\release\bowser.exe` (main @ cdf6ade3, `--features html-chrome-default`, `BOWSER_UI=html`).
Light scheme captured with the debug build + `BOWSER_FORCE_THEME=light` (release follows the system theme, dark here).
Method: `vg.ps1` in this folder; every launch used a fresh temp `--data-dir` + LOCALAPPDATA; only started PIDs were stopped.
Screenshots = CopyFromScreen of the union of Bowser's windows (menus/popups/hover included).
Menus verified HTML by window class (BowserHtmlPopup / BowserHtmlHover). Passwords and Settings are still native windows.

DPI: display is 100%. Real-app captures exist ONLY at 100%. 125/150/200% are HEADLESS renders of `ui/dist` (files 90-93), labelled as such.

## User requirements
| Requirement | Result |
|---|---|
| Horizontal row always filled | Pass with 1, 2, 4, 12 tabs, 700-1920px |
| Active tab widest | Pass (4 tabs 407 vs 275px; 12 tabs ~138 vs 100px) but hard to pick out at 12 (V17) |
| Address bar + suggestions as one surface | Partial: one surface, bottom radius matches top; but pill top half changes when open (V11) and bottom corners are aliased (V12) |

## Defects: 36 total (S1 10, S2 19, S3 7)
S1 = broken/unusable, S2 = visible design defect or native leftover, S3 = polish.

| ID | Sev | Surface | Defect | Shots | Owner file (likely) |
|---|---|---|---|---|---|
| V1 | S1 | Left panels (History/Bookmarks/Customize) | On a tab with a page already loaded, the page WebView is not shrunk: the panel is hidden behind it (1px edge). Works on new tabs only. | 45 | src/win32/side_panel.rs, apply_sidebar_bounds in window.rs |
| V2 | S1 | Privacy dashboard | Ctrl+Shift+Del/menu opens nothing visible; dialog host is created under existing children (6px sliver). Later opens silently ignored (open_privacy early return). | 34, 34b, 38 | src/win32/html_dialog.rs, privacy_panel.rs |
| V3 | S1 | Settings | HTML layer drawn over native GDI controls; overlapping text; window starts 16px above screen top. | 35 | settings_panel.rs, html_dialog.rs |
| V4 | S1 | Site info, command palette | See-through: page text shows through popup body (both schemes). | 14, 14b, 23, 62, 64 | styles/menu.css, tokens.css, popup host alpha |
| V5 | S1 | Side panel host | Stale host-painted "Panel"/"History" title + white wash stay over back/forward and sidebar top; eats clicks. | 30, 61-66 | side_panel.rs |
| V6 | S1 | Tooltips/hover card | After closing the main menu by clicking its button again, tooltips and tab hover cards never appear again that session (Esc close is fine). | 20, 21 | html_hover.rs tip(), menu toggle-close path in window.rs |
| V7 | S1 | Notes & Tasks | Native EDIT + bare "Delete" + "Note" box on both tabs; ghost "Tasks" label (old D1). | 31, 32 | notes_panel.rs, ui/src/panels/notes* |
| V8 | S1 | Passwords | Native GDI window; enabled-looking Reveal/Copy/Edit/Delete with zero entries; developer copy. (Policy: stays native for secrets; restyle/fix states + copy only.) | 33 | passwords_panel.rs |
| V9 | S1 | Customize > Create workspace | Clears field, creates nothing (rail "+" works). | 40, 41 | panels/customize.ts, handler in window.rs |
| V10 | S1 | Side panel host | After several open/close cycles: stacked empty frames until restart. | 39 | side_panel.rs |
| V11 | S2 | Omnibox | Opening the list changes the pill top half: focus ring 2px accent+inner -> 1px grey edge, edge moves in 1px, fill changes, lock gains backdrop. | 15, 16, 18 | styles/toolbar.css .pill.is-open |
| V12 | S2 | Omnibox list corners | Bottom corners clipped by a window region: aliased stair-steps vs anti-aliased top corners. | 17 | html_hover/suggest region in src/win32 |
| V13 | S2 | Vertical sidebar pinned | Stray "x" glyphs on both sides of pinned title; pinned rows are full rows (spec: 3-up 32px tiles). | 07, 09, 10b | sidebar.ts, sidebar.css |
| V14 | S2 | Toolbar (vertical mode) | Circular chevron button clipped above window top-right. | 07, 09 | toolbar.css / view.ts |
| V15 | S2 | Vertical sidebar | No collapsed 48px icon-only state; drag stops at 180px with bad truncation. | 09 | sidebar.ts, settings.rs |
| V16 | S2 | Horizontal tabs | Inactive titles hard-clipped mid-glyph, no ellipsis/fade. | 04b, 50, 51 | tabs.css |
| V17 | S2 | Horizontal tabs | Active tab differs only by ~1.4x width + faint fill; hard to find in dark. | 03, 04b | tabs.css |
| V18 | S2 | Tab groups | Auto group: grey dot + outlined pill; spec: coloured 2px underline + 6px dot, no pill chrome. | 15, 07, 60 | tabs.css, sidebar.css, tab_groups.rs |
| V19 | S2 | Workspace context menu | Opens left of the rail at screen x=0, outside the window; Move up/down enabled with one workspace. | 27 | popup placement in src/win32, window.rs |
| V20 | S2 | Rail workspaces | New workspaces get the same icon/no colour as Default. | 42, 43b | src/workspaces.rs |
| V21 | S2 | Command palette | Same command glyph on every row; tab rows lack favicons; blank row for untitled tab; headers butt against rows. | 23, 24, 64 | palette view + menu.css |
| V22 | S2 | Site info | Two-line rows have 0px between them; disabled footer on hairline. | 14b, 62 | menu.css (site-info) |
| V23 | S2 | Panels | Two headers + two close buttons (host-painted + HTML). | 30, 36, 37 | side_panel.rs, panels.css |
| V24 | S2 | History panel | Letter tiles instead of favicons. | 30, 66 | panels/history.ts |
| V25 | S2 | Customize panel | Classic Win32 scrollbar. | 37 | panels.css |
| V26 | S2 | New tab | Empty opaque surface (reads as broken); split-view new-tab pane near-black. | 06, 47, 66 | new-tab surface |
| V27 | S2 | Light scheme | Rail icons and inactive tab titles low contrast. | 60, 90-93 | tokens.css, rail.css, tabs.css |
| V28 | S2 | Narrow window | 700px + 12 tabs: 4-letter stubs; no favicon-only mode. | 51 | tabs.ts, tabs.css |
| V29 | S2 | Hover card | Plain tooltip flashes before card; mismatched marker shapes; stray bullet. | 22 | hover.css, hover.ts |
| V30 | S3 | Main menu drill-in | Width/left edge jumps on sub-page; footer repeats. | 10, 12 | menu-view.ts, menu.css |
| V31 | S3 | Import offer | Appears as an accidental-looking menu with paragraph text. | 01, 55 | import offer UI |
| V32 | S3 | Tab drag | Drop index wrong (lands after the group). | 44 | tabs.ts |
| V33 | S3 | Toolbar | New-tab "+" has no tooltip. | 21 | view.ts |
| V34 | S3 | Omnibox icon | Info glyph in empty/new-tab state; search glyph expected. | 06, 30 | view.ts |
| V35 | S3 | Focus | Pill keeps focus ring/caret after page load. | 05, 34 | window.rs focus |
| V36 | S3 | Tab context menu | "Mute site" uses the globe glyph, "Close other tabs" the trash glyph; opens over its own tab. | 13 | menu model/menu-view.ts |

## Interaction pass (real app, dark)
Pass: back/forward/reload/+, split menu, main menu, Esc closes every popup, menu keyboard nav + typeahead + drill-in, tab drag reorder, edge resize, double-click maximise, Win+Left snap, rail workspace switching.
Fail: Ctrl+H/bookmarks/customize on a loaded tab (V1), privacy dashboard (V2), Create workspace (V9), tooltips after toggle-close (V6).

## Not covered
Real-app 125/150/200% DPI; frame-by-frame animation; bottom-edge resize.
