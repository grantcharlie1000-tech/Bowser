# Split view: Chrome-style redesign

Owner: split view lane. Status: design accepted, implemented on `feat/split`. Audit and
verification results are at the end of this file. Product rules come from `PRODUCT.md`, tokens and
components from `ui/STYLEGUIDE.md`, and pixel targets from `docs/design/reference-spec.md`
("Split view": 8px page radius, 2px accent edge on the focused pane, 8px divider hit width).

## 1. What Chrome does (research, October 2026)

Sources: Google's help page "Use split view for multitasking in Chrome"
(support.google.com/chrome/answer/16971124), Chrome Story (Nov 2025), omgmog.net (Feb 2026),
Gadget Bond (Dec 2025), Zapier, Nerds Chalk. Split view shipped in Chrome 142-143 (desktop,
late 2025), after flag-only builds (`#side-by-side`) from Chrome 139. Screenshots are saved in
`docs/design/references/split/` (third-party images, for reference only):

| File | Shows |
|---|---|
| `gadgetbond-split.png` | Split pair as one joined tab, two rounded pages with a gap, thicker border on the active page, origin chip on the inactive page, split icon in the toolbar |
| `chromestory-reverse.png` | Toolbar split menu (Separate views, Close left view, Close right view, Reverse views) and the empty side: "Choose a tab to add to split view" with tab cards and an X |
| `chromestory-open-split-view.png`, `omgmog-tab-menu.png` | Tab context menu: "New split view with current tab" |
| `chromestory-link-menu.png` | Link context menu: "Open link in split view" |
| `omgmog-split.png`, `omgmog-status-menu.png` | Per-page chip menu: Separate views, Close this view, Reverse views |
| `omgmog-toolbar-menu.png` | Toolbar split button pressed, menu items with icons |
| `google-icon-*.png` | Help-page icons: split, left active, right active |

Behaviour summary:

- **The split is a tab pair, not a window mode.** The two tabs become one joined tab in the strip
  (both favicons and titles, one close button each). Selecting another tab shows that tab alone;
  selecting either half shows the split again with that side active.
- **Entry points.** Tab menu "New split view with current tab" (on an inactive tab: pairs it with
  the active tab) and "Add tab to new split view" (on the active tab: opens an empty side);
  link menu "Open link in split view"; drag a tab to the left or right edge of the page (a
  "Create split view" placeholder with a plus appears); a pinnable toolbar button; Shift+Alt+N
  (Windows) opens split view from the active tab.
- **Empty side.** A full-height page "Choose a tab to add to split view" listing the window's other
  tabs as cards (favicon, title, host); the address bar also works there. An X closes that side.
- **Layout.** Two rounded pages with a small gap (about 8 DIP). The active page has a thicker,
  brighter border; the inactive page a light one. Clicking a page (or scrolling it is allowed
  without activating) makes it active. The divider can be dragged; panes keep a minimum width.
  Chrome remembers the position for the session; double-click on the divider swaps sides.
- **Toolbar.** Back, forward, reload, the address bar, Translate, Print and zoom act on the active
  page only; the split icon in the toolbar shows which side is active (left/right half filled).
- **Menus.** Toolbar split icon and tab menu: Separate views, Close left view, Close right view,
  Reverse views (plus stacked/side-by-side in newer builds). Per-page chip: Separate views, Close
  this view, Reverse views.
- **Closing.** Closing either tab of the pair ends the split; the other tab stays as a normal tab.
  Separate views keeps both tabs.

## 2. Bowser design

### 2.1 Model (`src/split_view.rs`, `src/app.rs`)

- `SplitView { left, right, focused, ratio }` stays the pair. New pure helpers: `separate()`
  (focused tab survives as the active single view), `close(pane)` (the tab to close and the
  survivor), `ratio_for_divider()` and `divider_for_ratio()` (integer DIP-safe rounding),
  `MIN_RATIO`/`MAX_RATIO` 20..80 and `MIN_PANE_DIP` 240, `fill_empty(empty, picked)` (the empty
  side takes the picked tab, the empty tab is discarded).
- The app keeps **one pair per window**. While a tab outside the pair is selected the pair is
  *parked* (`split_parked`), exactly like Chrome showing the other tab alone; selecting either
  member un-parks it with that side focused. New tab, tab switch, workspace switch and Ctrl+Tab
  therefore never replace a pane any more. Closing either member dissolves the pair.
- Opening a pair moves the right tab directly after the left one in the tab model, so both strips
  show the halves together.
- A small in-memory ring of recently closed tabs (http(s) only, never private tabs, 8 entries,
  never written to disk) feeds the empty side's "Recently closed" list.
- Crash recovery keeps saving the shown pair (left, right, focused side, ratio), unchanged format.

### 2.2 Layout and divider (native window + overlay)

| Item | Value | Token / source |
|---|---|---|
| Page gap (divider width and hit width) | 8 DIP | `--bw-space-2`, reference-spec 8px hit |
| Page corner radius | 9 DIP, both panes, all four corners | `--bw-r-tab` (same as a single page) |
| Active pane edge | 2 DIP accent ring inside the rounded edge | `--bw-accent` (`omnibox_focus_border`) |
| Inactive pane edge | 1 DIP hairline | `--bw-border` (`omnibox_border`) |
| Grip | 4 x 32 DIP pill centred in the gap | `--bw-r-pill`; idle text 22% (`--bw-pill-edge`), hover text 60%, drag accent |
| Minimum pane width | 240 DIP; below 2 x 240 + 8 the split collapses to the focused pane | |
| Ratio | 20-80%, stored per pair, saved with crash recovery | |

The edges, gap and grip are drawn by the existing layered chrome overlay, which sits above the
WebView2 child windows and is transparent to input, so nothing covers or delays page input. The
gap itself is the main window's client area, so hit-testing it needs no extra window. A page that
swallows its own clicks still reports focus through WebView2 `GotFocus`, which focuses its pane.

Divider input: drag resizes (bounds are applied at most once per animation frame); a click on the
grip without moving opens the divider menu; a right-click anywhere on the divider opens the same
menu; double-click restores 50/50 (kept from Bowser 1.x and documented; Chrome swaps instead).

Divider menu (HTML popup menu, `menu::show`), anchored beside the grip:
Swap sides (`swap`), Close left tab (`split-left`), Close right tab (`split-right`),
separator, Separate views (`unsplit`).

### 2.3 HTML surfaces

**Empty side (`?view=split`, `View::Split`, `ui/src/split.ts`, `split.css`).** A dedicated chrome
host created lazily the first time a split has an empty side, parented to a Bowser-owned child
window that exactly covers the empty pane (no page is under it, so there is no HWND airspace
conflict). Hidden and `TrySuspend`ed when no empty side is shown.

| Part | Spec |
|---|---|
| Surface | opaque `--bw-window`, rounded 9 DIP by the overlay's pane mask |
| Column | max 520px, centred, top 12% (min 24px), `tw:gap-4` |
| Heading | Subtitle 20/28 strong: "Choose a tab for this side" |
| Close | `.bw-icon-btn.is-lg` top-right, 12px inset, `close` glyph, "Close this side" |
| Search | `.bw-input` 32px with a `search` glyph: "Search tabs or enter an address" |
| Lists | "Open tabs" and "Recently closed" `.bw-section-header`; rows `.bw-row` 32px with 16px favicon (globe fallback), title, host in `--bw-text-2`; selected row `--bw-selection` wash |
| Empty | "No other tabs in this workspace" Caption in `--bw-text-2` |

Keyboard: the search field is focused when the side appears; typing filters (title and host,
case-insensitive); Up/Down move the highlighted row (`aria-activedescendant`), Enter picks it, or
with no matching row navigates the side to the typed address/search (Rust normalises it as the
omnibox does); Escape closes the empty side. Rows are `role="option"` in a `role="listbox"`; the
search field is a `role="combobox"`.

**Drop-zone preview.** While a tab from the HTML strip is dragged below the strip over the page,
the same host shows a placeholder card over the target half: accent-tinted surface, 2 DIP accent
edge, `plus` glyph and "Open in split view" (Body strong) with the tab title (Caption). Rust
raises its container above the page's WebView2 window while it shows (and falls back to the
overlay tint when the host is not ready).

**Tab strip and vertical sidebar.** A `pair` message (Rust to tabs and toolbar views) names the
two tabs and the focused side. `ui/src/split-logic.ts` places the right tab directly after the left
one (same group and pin state for display) and the two tabs render as one joined capsule:

| Item | Spec |
|---|---|
| Halves | no gap between them; outer corners `--bw-r-tab`, inner corners 3px; 1px `--bw-hairline` seam |
| Shown pair | both halves get the island fill; the focused half keeps the active styling |
| Width | shown pair: each half flex 1.25 (pair 2.5, the widest unit); parked pair: each half 0.7 (pair 1.4, narrower than the active tab's 1.5) |
| Sidebar | the two rows join vertically into one capsule (inner corners 3px, seam hairline) |
| A11y | each half stays a `role="tab"`; its name gains ", left side of split view" or ", right side of split view" |

**Toolbar button.** `aria-pressed` follows the shown split; the glyph shows the focused side
(`split-left` / `split-right`, otherwise `split`). Click: not split, open a split with an empty
right side; split, open the split menu (Separate views, Swap sides, Close left tab, Close right tab).

### 2.4 Entry points

| Entry | Result |
|---|---|
| Toolbar split button / Ctrl+Shift+\ | Not split: active tab left, empty side right. Split shown: button opens the split menu; the shortcut separates views |
| Tab menu on an inactive tab: "New split view with current tab" | Active tab left, this tab right, right focused |
| Tab menu on the active tab: "Add tab to new split view" | Active tab left, empty side right |
| Tab menu on a tab of the pair | "Separate views", "Swap sides" |
| Link context menu: "Open link in split view" | New tab with the link beside the current tab |
| Drag a tab below the strip to the left or right half | Dragged tab on that side, the active tab (or an empty side) on the other |
| Divider grip / right-click | Divider menu |

Keyboard (unchanged bindings): Ctrl+Shift+\ toggle, Ctrl+Shift+Left/Right focus a side,
Ctrl+Shift+W close the focused side's tab.

### 2.5 Toolbar follows the focused side

The focused side's tab is the app's active tab, so back/forward/reload, the address text,
security chip, zoom chip, private chip, reader state and the window title all follow it. Focus moves
on: a click in either page (WebView2 `GotFocus`), a click on the gap's side of a pane, the picker,
Ctrl+Shift+Left/Right, or selecting either half in the strip.

### 2.6 Motion (transform and opacity only; nothing waits for it)

- Empty side: its column enters with `.bw-enter.is-scale` (opacity 0 to 1, .98 to 1, 167ms out-ease).
- Drop-zone card: `.bw-enter.is-scale` when it appears; it leaves at once (no exit animation, so a
  drop is never visually delayed).
- Tab strip: new halves use the strip's existing grow-in; joining reuses the strip's FLIP.
- Divider grip: colour change on hover and drag (overlay repaint, no timer).
- Reduced motion: the base-layer rule reduces every transition to an 83ms fade; `.bw-enter`
  becomes a plain fade.

### 2.7 Accessibility

- Every control is a real button/input with an accessible name; the picker is a labelled region
  ("Choose a tab for the right side") with a combobox and listbox.
- Focus ring: the shared `.bw-focus-ring` on the picker's buttons; rows show the selection wash.
- Contrast: titles `--bw-text` (>= 4.5:1, guaranteed by `theme.rs`), meta `--bw-text-2`; the
  active pane accent edge is >= 3:1 against the window colour (accent tokens are validated).
- Keyboard: everything in section 2.4 has a keyboard path (shortcuts, the toolbar button via Tab,
  the tab menu via Shift+F10 on a focused tab, the picker fully).

### 2.8 Dropped or deferred

- Inactive-pane dimming (would need a full-pane alpha wash in the overlay on every repaint and
  lowers the inactive page's legibility; the edge colour carries the state instead).
- Chrome's per-page origin chip with its own menu (the overlay is input-transparent; the divider
  grip and the toolbar button carry the menu instead).
- Stacked (top/bottom) split, more than one pair per window, the "pin split button" setting, and
  the setting that disables drag-to-split.
- Keyboard resizing of the divider.
- Persisting a parked pair across crash recovery (only the shown pair is saved, as before).
- The native split button and native drop tint stay only for the native (GDI) chrome; in HTML mode
  the native split button paint is skipped and the HTML surfaces replace the drop tint.
