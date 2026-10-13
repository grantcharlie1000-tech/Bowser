# Bowser independent visual-gate review

## 1. Verdict and surface scores

**FAIL — do not ship this visual gate. Overall: 4/10.**

The basic browser shell has a credible, restrained identity. The panel and overlay system does not look production-ready. Page text shows through security controls and the command palette; panels disappear under content; old controls, labels and frames remain painted over new states. These are broken surfaces, not matters of taste. Fix ownership, visibility, bounds and compositing before spending time on cosmetic refinements.

**22 findings: 5 S1 broken, 11 S2 clear design defects, 6 S3 polish.** IDs represent distinct visible failures or design problems, not proven independent code causes.

Method: independent, single-reviewer inspection of every one of the **68 supplied PNG files**, including every zoom crop and all four headless DPI sheets. Product criteria came from `PRODUCT.md` and the explicit user requirements. No other critique/report was opened. This is a screenshot review, not a source audit or live interaction test. Implementation suggestions below are proposed remedies, not diagnosed code causes. Native-looking rendering does not by itself prove a particular Win32/GDI implementation. No runtime, speed, keyboard-operation or measured WCAG-compliance claim is made.

Scores describe the captured appearance, including broken states. A high score for an individual component does not override a failed integration state elsewhere.

| Surface | Score /10 | Assessment |
|---|---:|---|
| Main window, toolbar, glass and page frame | 8 | Quiet proportions; page gets nearly all the space. Stale panel paint contaminates later states. |
| Horizontal tab allocation | 8 | Fills the usable row; selected tabs receive more width. Group styling weakens selection hierarchy. |
| Vertical tabs and group layout | 6 | Useful indentation and compact rows; pinned-label collision and clipped corner control. |
| Workspace rail | 6 | Compact and aligned, but identical workspace icons force recall. |
| Address pill and suggestions | 6 | Visually joined, readable suggestions; fails the invariant top-half requirement. |
| Main menu and workspace/profile submenus | 8 | Coherent row rhythm, shortcuts, restrained highlights and clear focus outline. Minor page bleed remains. |
| Tab/workspace context menus and split menu | 7 | Consistent custom styling; tab menu has faint content bleed over the Rust heading. |
| Site information and permissions | 2 | Page lettering collides with permission/status controls in both themes. |
| Command palette | 2 | Content bleeds through the entire list; repeated generic symbols weaken scanning. |
| History and bookmarks panels | 3 | Readable when visible; duplicate shells/titles and history hidden under loaded content. |
| Notes and Tasks | 2 | Empty-state message contradicts surviving editor/Delete controls, including on Tasks. |
| Passwords and passkeys | 4 | Readable, but looks like a separate legacy dialog with different typography and controls. |
| Privacy dashboard | 1 | Missing in one capture; only clipped slices visible in another. |
| Settings | 1 | Two incompatible layouts painted together; very low-contrast underlying labels and clipped title. |
| Customize | 5 | Reasonable grouping; wrong scrollbar treatment, redundant panel wrapper and truncated help. |
| New-workspace prompt | 8 | Clear title, focused input and restrained action hierarchy. |
| First-run import offer | 7 | Compact and dismissible in presentation, but explanation follows the decisions. |
| New-tab and split-view states | 6 | Normal new-tab blankness suits the brief; split pane changes to a black, unexplained empty surface. |
| Tooltips and tab hover card | 6 | Reload tooltip is useful; hover card gives colour controls too much visual priority. |
| Narrow, snapped and maximised shell | 8 | Captured toolbar and tab strip remain contained; narrow suggestions lose useful identifying text. |
| Light/dark consistency | 4 | Core token mapping is recognizable; light captures expose ghost regions and weak fallback-icon contrast. |
| Headless DPI component rendering | 8* | Basic proportions scale consistently across 100/125/150/200%; *does not certify native-window DPI behaviour*. |

### Explicit requirement (a): full-width horizontal tabs; selected tab widest

**PASS in the supplied static states, with a group-header qualification.**

`02-tabs-1-example.png` and `05-page-wikipedia-1-tab.png` use the full tab area from the rail to the right window inset. `03-tabs-4.png` subdivides that same area, with the active Rust tab visibly wider than its peers. `04-tabs-12.png` / `04b-tabs-12-strip-zoom.png` continue filling the row with the active BBC tab wider than individual inactive tabs. `06-new-tab-page.png`, `43-rail-switched-to-default.png` and `47-split-view.png` show the width priority moving to the selected tab. The row remains filled in `50-window-wide-1600x900.png`, `51-window-narrow-700.png`, `53-window-snapped-left-half.png` and `54-window-maximised-double-click.png`.

The fixed-looking `wikipedia.org` **group header** is wider than the active BBC tab in `51-window-narrow-700.png`. It is not an individual tab, so this does not disprove the stated selected-*tab* rule, but it does become the most prominent island at the tightest width. If the intended rule includes every group header too, that narrow state fails. The normal gaps between islands and reserved rail are legitimate; there is no large unused tail. Static evidence cannot establish “always” during dragging, animation or arbitrary tab counts.

### Explicit requirement (b): one expanding address surface; unchanged top half

**FAIL.** `16-address-suggestions-open.png` and `19-address-suggestions-arrow-down.png` do successfully read as one vertically continuous surface: no detached popup gap, no doubled rounded seam below the input. But `18-pill-top-half-closed-vs-open.png`, comparing `15-address-focused.png` with `16-address-suggestions-open.png`, shows the pale focus perimeter disappearing and the input fill changing from cool grey to the darker popup material. The upper contour/edge treatment is visibly redrawn. Opening the list changes more than the lower corners.

`17-suggest-corners-zoom.png` shows rounded, approximately comparable upper and lower corner curves in the *open* state; I do **not** see evidence for a gross bottom-corner radius mismatch. The different page backdrop and edge contrast make exact pixel equivalence unsafe to claim visually. Matching the bottom corners of the replacement box is insufficient when the original pill's upper outline and fill have changed. Preserve the original focused input's position, radius, fill, top/side stroke and icon alignment, extend its background downward, and relocate only its lower termination. Validate by registered closed/open top-half pixel comparison with the same input text and focus state; compare mirrored corner masks separately. Light mode needs the same check (`63-light-address-suggestions.png`).

## 2. Prioritised defect table

Severity: **S1 = broken**, **S2 = clear design defect**, **S3 = polish**. Within each severity, fix top to bottom.

| ID | Severity | Surface | Precise finding | Screenshot evidence | Concrete fix suggestion |
|---|---|---|---|---|---|
| A1 | S1 | Site info and command palette | Unblurred page text remains legible through popup bodies and runs directly across their text and controls. The Rust headline crosses tracker status/reset permissions; Wikipedia copy crosses command results. Light mode retains the same problem. These surfaces cannot provide stable reading contrast. Ordinary menus exhibit a weaker version. | `14-site-info-popup.png`, `14b-site-info-zoom-see-through.png`, `23-command-palette.png`, `24-command-palette-filtered.png`, `62-light-site-info.png`, `64-light-command-palette.png`; milder in `13-tab-context-menu.png`, `61-light-main-menu.png` | Give interactive popup bodies a solid or sufficiently occluding theme surface. Keep glass in the window material; do not transmit sharp page text through controls. Verify above dense black text, coloured imagery and both page themes. Inspect native/WebView compositing as well as CSS opacity. |
| A2 | S1 | Privacy/history panel visibility | Privacy has no readable dashboard in its nominal capture; another shows only narrow slices of its labels at the left edge of the main page frame. History is absent over the loaded Wiki page despite a panel boundary at approximately x=374. The panel/content bounds and stacking disagree. | `34-panel-privacy-dashboard.png`, `34b-privacy-dashboard-zoom-sliver.png`, `38-customize-overlapped-by-privacy.png`, `45-history-panel-hidden-behind-loaded-page.png` | Establish one owner for the panel rectangle and page rectangle. Resize the actual page host to the remaining area and place panel content above its backing surface. Verify open, load page, switch tab, change theme, resize and close. A CSS z-index alone may not fix a separate native child surface. |
| A3 | S1 | Panel lifecycle | Several empty rounded frames survive at different x positions, producing a stack of blank sheets with no usable panel content. The frame edges are not an intentional multi-pane layout. | `39-panels-wedged-ghost-frames.png` | On panel replacement/close, remove the previous content and frame together, release its reserved width, and repaint the vacated area. Keep a single active panel shell per intended slot; test repeated panel switching on blank and loaded tabs. |
| A4 | S1 | Notes/Tasks state | “No notes yet” coexists with a title-like field, Delete action and Note editor at the bottom. After selecting Tasks, the same Note editor and Delete action remain beneath “No tasks yet.” A faint extra Tasks label and extra close mark are also visible. | `31-panel-notes-tasks.png`, `32-panel-notes-tasks-tab.png` | Render exactly one selected section and one empty/editor state. Unmount or explicitly hide the previous editor and any legacy native children when switching to Tasks or no selection. Keep destructive actions tied to a visible selected object. |
| A5 | S1 | Settings | Modern Search/Tabs/Blocking navigation and search-engine choice sit over another form containing dropdowns, checkbox rows, numeric values and a second bottom action row. Underlying labels are nearly black on dark grey. The upper Settings title is clipped by the capture edge and the panel extends beyond the browser body. This reads as two settings windows superimposed. | `35-panel-settings.png` | Choose one settings renderer and remove/hide its predecessor as a complete unit. Lay out one header, one active section and one footer; constrain the dialog to available work area with internal scrolling. Restore readable label/helper colours and consistent controls. |
| A6 | S2 | Address expansion | The top-half focus outline and fill change when suggestions open, violating the explicit invariant even though the input/list join is continuous. | `15-address-focused.png`, `16-address-suggestions-open.png`, `17-suggest-corners-zoom.png`, `18-pill-top-half-closed-vs-open.png`, `19-address-suggestions-arrow-down.png` | Preserve a shared outer surface and fixed input rendering. Extend only the lower portion; keep the upper perimeter and focus treatment unchanged, and use the original upper radius for the final lower corners. Compare matched-state pixels, not only CSS radius values. |
| A7 | S2 | Passwords dialog / legacy visual language | Square window silhouette, visibly different text rendering with a serif-looking heading, teal outlined controls and bright blue primary button do not belong to the restrained rounded menu/prompt family. The isolated action bank floats below a large empty region. | `33-panel-passwords.png`, compared with `41-new-workspace-prompt.png`, `10-main-menu.png` | Rebuild the dialog presentation with the shared Segoe UI Variable type scale, dialog radius, spacing and button tokens. Use the established muted accent and place empty-state creation/import actions near the empty-state explanation. Preserve truthful security limitations. |
| A8 | S2 | Panel wrappers | History gets two headings and two close buttons; bookmarks gets a generic “Panel” heading followed by Bookmarks and another close button. Customize also retains the generic wrapper. The extra 40–50 px of header spends page space without adding meaning. | `30-panel-history.png`, `36-panel-bookmarks.png`, `37-customize-panel.png`, `65-light-vertical-sidebar.png`, `66-light-new-tab-page.png` | Let the shell own one meaningful title and one close action; panel bodies begin with their search or content. Eliminate placeholder “Panel” copy and duplicate close controls. |
| A9 | S2 | Vertical mode caption controls | A rounded down-chevron control peeks out at the extreme top-right with most of its shape above the visible window; it crowds the close control below. This looks clipped, not like a deliberate caption affordance. | `07-vertical-sidebar-expanded.png`, `08-vertical-sidebar-group-collapsed.png`, `09-vertical-sidebar-narrowest.png`, `10b-sidebar-pinned-row-and-corner-chevron-zoom.png`, `65-light-vertical-sidebar.png` | Position the control inside a defined caption slot with the same centreline and inset as its neighbours, or remove it in vertical mode if redundant. Include this control in caption bounds and DPI checks. |
| A10 | S2 | Pinned vertical tab | The narrow pinned Example Domain row allows a close mark into the end of the title while a pin also occupies the trailing area. The zoom shows the title crowded by action glyphs rather than cleanly truncated text. | `09-vertical-sidebar-narrowest.png`, `10b-sidebar-pinned-row-and-corner-chevron-zoom.png` | Reserve fixed trailing action space before measuring the label. Ellipsize within the remaining width; give pin/close separate cells or expose one action on hover without covering the label. |
| A11 | S2 | Stale chrome paint / light theme | “Panel” or “History” text appears over the back/forward area, with stray character fragments near Home. In light mode, pale rectangular regions cross the toolbar and tab strip; History also overlaps the vertical group's label. These are visibly unrelated to their current locations. | `30-panel-history.png`, `34b-privacy-dashboard-zoom-sliver.png`, `39-panels-wedged-ghost-frames.png`, `61-light-main-menu.png`, `63-light-address-suggestions.png`, `65-light-vertical-sidebar.png`, `66-light-new-tab-page.png` | Clear or destroy obsolete title/native-child surfaces when their host moves or closes. Recompute panel origin below chrome and invalidate old bounds on theme/orientation changes. Never let panel header backgrounds paint in the navigation strip. |
| A12 | S2 | Customize scrollbar | A bright white, arrow-ended classic scrollbar cuts down the dark Customize panel. It is a conspicuous native-looking leftover inside app-owned UI, unlike the quieter dark page scrollbar. | `37-customize-panel.png`, `40-customize-create-workspace-no-effect.png` | Theme the app-owned scroll area with a narrow neutral track/thumb matching the panel; retain discoverability and usable hit area. Do not confuse this with third-party webpage controls. |
| A13 | S2 | Workspace identity / rail icons | Default and Research are represented by identical briefcase icons. Selection is a subtle side mark and background; the inactive workspace has no visible identity. The tiny boxed-plus and grid-plus below add another pair of similar creation metaphors. | `42-rail-workspace-2-active.png`, `43-rail-switched-to-default.png`, `43b-rail-zoom.png` | Give workspaces a persistent distinguishing initial, restrained colour marker or chosen icon. Use distinct add-workspace/add-web-app symbols and explicit tooltips; preserve a clear selected indicator without relying only on colour. |
| A14 | S2 | Command palette icon semantics | New tab, Close tab, Settings and Notes all use the same command-symbol glyph; open-tab rows repeat a generic grid-like symbol rather than the site's favicon. The menu already has more discriminating icons for these actions. The result is a repetitive icon column with little scanning value. | `23-command-palette.png`, `24-command-palette-filtered.png`, `64-light-command-palette.png`, compared with `10-main-menu.png` | Reuse the main menu's action icons and real/fallback site favicons for tab results. Keep one icon box, stroke family and baseline across categories; reserve category symbols for headings. |
| A15 | S2 | Light-theme fallback icons | Sidebar fallback globes are white on pale grey chips, markedly weaker than dark toolbar icons and actual favicons. The same washed-out fallback treatment repeats in the headless light-theme sheets. Distinct icon sources do not have comparable visual weight/contrast. | `65-light-vertical-sidebar.png`, `90-headless-dpi-125.png`, `91-headless-dpi-150.png`, `92-headless-dpi-200.png`, `93-headless-dpi-100-reference.png` | Make fallback glyphs use the light theme's dark foreground; use shared icon bounds and theme-aware chip fills. Measure non-text contrast on actual composited backgrounds and balance optical size against real favicons. |
| A16 | S2 | Group versus selected-tab hierarchy | The collapsed wikipedia.org group uses a bold label, prominent grey island and outlined treatment resembling an active tab, while BBC is the actual active page. At narrow width the group is the largest island, further weakening the active-tab signal. | `44-tab-drag-reorder-result.png`, `50-window-wide-1600x900.png`, `51-window-narrow-700.png`, `53-window-snapped-left-half.png` | Separate group-header styling from selection: smaller marker, restrained fill, explicit disclosure indicator; reserve the strongest fill/outline for the active tab. Include group widths in narrow-layout budgeting. |
| A17 | S3 | Narrow suggestions | At 700 px, Rust and Tab result titles and paths are both abbreviated while “Switch to tab” retains substantial width. The list fits, but spends scarce width on the action phrase instead of distinguishing the results. | `52-window-narrow-700-suggestions.png` | At the narrow breakpoint, put domain/path on a second muted line or abbreviate the action badge. Prioritise a recognizable title and origin; preserve the full text in an accessible name. |
| A18 | S3 | Empty split pane | The selected empty split pane is nearly black, unlike the slate normal New tab background, and contains no instruction or visible exit affordance. The active outline is clear, but the empty pane looks like an unpainted content host. | `47-split-view.png`, compared with `06-new-tab-page.png`, `46-split-view-button.png` | Use the same New tab background in single and split modes; add a compact contextual hint or pane action for choosing a page/closing the split. Keep the initial state quiet. The screenshot does not prove navigation or dismissal is broken. |
| A19 | S3 | Import offer hierarchy | Import / Not now / Don't ask again appear before the explanation and without a contextual heading. The user is asked to choose before learning what will be imported. | `01-import-offer-first-run.png`, `55-window-default-size-1000x720-import-offer.png` | Put a short “Import from another browser” heading and scope sentence first, then the primary action and dismiss choices. Preserve the compact size and accurate limited import scope. |
| A20 | S3 | Tab hover card | Five large saturated colour circles and a plus dominate a small title/domain preview. The controls attract more attention than the identity of the tab and their purpose is not explicit in the card. | `22-tab-hover-card.png` | Lead with favicon, title and domain. Place colour/group assignment in a smaller labelled secondary row or an explicit grouping control; retain restrained accents until the user chooses a colour. |
| A21 | S3 | Customize helper text | The History explanation ends “sid...” even though the panel has ample vertical space. A useful instruction is truncated merely to maintain a single line. | `37-customize-panel.png`, `40-customize-create-workspace-no-effect.png` | Allow helper copy to wrap within the text column, keep the toggle aligned to the primary label, and use a consistent row gap. |
| A22 | S3 | History favicon fallback | Wiki, Microsoft Edge and WebView entries use E tiles rather than the recognizable Wikipedia W favicon seen in the tab strip and suggestions. A hostname-derived E is particularly unhelpful across several Wikipedia entries. | `30-panel-history.png`, `65-light-vertical-sidebar.png`, `66-light-new-tab-page.png`, compared with `16-address-suggestions-open.png` | Reuse cached site favicons; when unavailable, use a consistent neutral globe or a meaningful site-name initial. Do not derive the initial from an uninformative language subdomain. |

### Evidence boundaries and DPI assessment

- `40-customize-create-workspace-no-effect.png` looks essentially like `37-customize-panel.png`, with an empty workspace-name field and no feedback. That does **not** prove the click was delivered or that valid creation fails. No separate functional defect is counted from the filename. Test valid input, blank validation, success feedback and rail update after the visual blockers are fixed.
- `21-new-tab-button-hover-no-tooltip.png` shows a hover background but no tooltip, while `20-tooltip-reload.png` shows “Reload (Ctrl+R).” Hover duration is unknown. Tooltip consistency needs a timed check; this is not counted as a proven missing-tooltip bug.
- `90-headless-dpi-125.png`, `91-headless-dpi-150.png`, `92-headless-dpi-200.png` and `93-headless-dpi-100-reference.png` all show recognizable, proportionally consistent toolbar, rail, tab and menu rendering. No obvious scale-specific icon deformation or baseline collapse is visible. The light fallback-icon weakness persists across scales (A15).
- Those sheets clip the right edge of sidebar rows/long titles and the history search field at the fixture boundary, including at 100%. Treat that as inadequate capture coverage, not proven DPI-only app clipping. Recapture full component bounds before asserting sidebar ellipsis/right-padding correctness. The purple sheet gutters are presentation background, not Bowser chrome defects.
- Headless rendering with a fake host cannot certify Win32/WebView bounds, real popups, monitor transitions, caption positioning or glass contrast over varied desktop backdrops. The clean sheets do not outweigh broken real-app panels.
- Native-looking webpage forms, typography and scrollbars belong to the website unless evidence shows Bowser ownership. The browser-owned Customize scrollbar and settings/password presentation are the relevant anti-reference failures. Favicon brand artwork is not required to match the browser's action-icon family.
- Blank New tab by itself is consistent with “quiet” and is not counted as a failure. Screenshots cannot prove input latency, drag correctness, Escape handling, focus order or screen-reader semantics.

## 3. Five things done well

1. **The shell gives space back to the page.** The shallow toolbar, slim rail and low tab row avoid Electron-like bulk. The wide and default windows keep the content dominant (`05-page-wikipedia-1-tab.png`, `50-window-wide-1600x900.png`, `56-window-default-size-1000x720.png`).
2. **The horizontal width allocation is recognizably the requested design.** One tab spans the row; additional tabs subdivide it; the selected tab gets more room. The arrangement survives the shown 700 px and maximised states (`02-tabs-1-example.png`, `03-tabs-4.png`, `04-tabs-12.png`, `51-window-narrow-700.png`, `54-window-maximised-double-click.png`).
3. **The custom menu family is largely coherent.** Row spacing, separators, icon column, shortcut alignment and modest rounding look deliberate. The visible keyboard focus ring is especially clear (`10-main-menu.png`, `11-main-menu-keyboard-nav.png`, `12-main-menu-workspaces-submenu.png`, `26-profile-menu.png`, `26b-profile-menu-standalone.png`, `27-workspace-context-menu.png`).
4. **Vertical groups communicate structure without large cards.** Indentation, a thin group guide, count and disclosure distinguish the group from its children; the collapsed capture saves space cleanly (`07-vertical-sidebar-expanded.png`, `08-vertical-sidebar-group-collapsed.png`). Preserve this structure while fixing the pinned row and caption control.
5. **The suggestions list already has the right basic composition.** It joins directly to the address input, presents title plus origin, distinguishes “Switch to tab,” and shows a clear selected result. This is a solid basis for fixing the top-half invariant rather than redesigning the interaction (`16-address-suggestions-open.png`, `19-address-suggestions-arrow-down.png`, `63-light-address-suggestions.png`).

### Inspection coverage

Every file below was individually opened and visually inspected; no contact sheet was substituted for viewing the supplied images.

```text
01-import-offer-first-run.png
02-tabs-1-example.png
03-tabs-4.png
04-tabs-12.png
04b-tabs-12-strip-zoom.png
05-page-wikipedia-1-tab.png
06-new-tab-page.png
07-vertical-sidebar-expanded.png
08-vertical-sidebar-group-collapsed.png
09-vertical-sidebar-narrowest.png
10-main-menu.png
10b-sidebar-pinned-row-and-corner-chevron-zoom.png
11-main-menu-keyboard-nav.png
12-main-menu-workspaces-submenu.png
13-tab-context-menu.png
14-site-info-popup.png
14b-site-info-zoom-see-through.png
15-address-focused.png
16-address-suggestions-open.png
17-suggest-corners-zoom.png
18-pill-top-half-closed-vs-open.png
19-address-suggestions-arrow-down.png
20-tooltip-reload.png
21-new-tab-button-hover-no-tooltip.png
22-tab-hover-card.png
23-command-palette.png
24-command-palette-filtered.png
26-profile-menu.png
26b-profile-menu-standalone.png
27-workspace-context-menu.png
30-panel-history.png
31-panel-notes-tasks.png
32-panel-notes-tasks-tab.png
33-panel-passwords.png
34-panel-privacy-dashboard.png
34b-privacy-dashboard-zoom-sliver.png
35-panel-settings.png
36-panel-bookmarks.png
37-customize-panel.png
38-customize-overlapped-by-privacy.png
39-panels-wedged-ghost-frames.png
40-customize-create-workspace-no-effect.png
41-new-workspace-prompt.png
42-rail-workspace-2-active.png
43-rail-switched-to-default.png
43b-rail-zoom.png
44-tab-drag-reorder-result.png
45-history-panel-hidden-behind-loaded-page.png
46-split-view-button.png
47-split-view.png
50-window-wide-1600x900.png
51-window-narrow-700.png
52-window-narrow-700-suggestions.png
53-window-snapped-left-half.png
54-window-maximised-double-click.png
55-window-default-size-1000x720-import-offer.png
56-window-default-size-1000x720.png
60-light-tabs-4-wikipedia.png
61-light-main-menu.png
62-light-site-info.png
63-light-address-suggestions.png
64-light-command-palette.png
65-light-vertical-sidebar.png
66-light-new-tab-page.png
90-headless-dpi-125.png
91-headless-dpi-150.png
92-headless-dpi-200.png
93-headless-dpi-100-reference.png
```
