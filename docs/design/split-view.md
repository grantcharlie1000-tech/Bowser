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

- `SplitView { left, right, focused, ratio, orientation }` is the pair. `orientation` is
  `Orientation::SideBySide` (left | right, the default) or `Orientation::Stacked` (top over bottom).
  The slots keep their names in both: `left` / `Pane::Left` is the *first* pane (left, or top when
  stacked), `right` / `Pane::Right` the *second* (right, or bottom); `ratio` is the first pane's
  share of the split axis (width, or height). This kept every caller of the pair unchanged; only
  geometry, wording and the wire's edge names look at the orientation. `turn()` switches the
  orientation (first stays first, focus and ratio stay). New pure helpers: `separate()`
  (focused tab survives as the active single view), `close(pane)` (the tab to close and the
  survivor), `ratio_for_divider()` and `divider_for_ratio()` (integer DIP-safe rounding),
  `MIN_RATIO`/`MAX_RATIO` 20..80 and `MIN_PANE_DIP` 240, `fill_empty(empty, picked)` (the empty
  side takes the picked tab, the empty tab is discarded).
- **Up to four panes (October 2026).** Each half of the pair may be divided once across the
  primary split: `trail: [Option<TabId>; 2]` holds the second cell of the first / second half
  (`Pane::LeftTrail`, `Pane::RightTrail`; the half's old slot is its *lead* cell: top of a column,
  left of a row) and `cross_ratio` is the lead cells' share of the cross axis, shared by both
  halves so a 2 x 2 is a true grid whose dividers cross in one point. This small tree of depth two
  covers every layout a corner drop can reach: a pair, the four "T" layouts (one half divided) and
  the 2 x 2. Chosen over a free binary tree (which would also allow three columns, nested
  dividers that never line up, and drop rules nobody can predict) and over a bare 2 x 2 grid with
  empty slots (which loses which divider is full length, so closing a pane could not undo the drop
  that made it). With no trail the struct is exactly the old pair, so every pair rule, saved line
  and `pair` message is unchanged. `SplitView::cells()` maps any layout onto the page's quadrants
  (`Quadrant`), which is what drops, focus moves and pane names are decided on;
  `SplitView::from_cells` builds a layout back (primary split along the preferred axis when the map
  allows it). `without(tab)` closes a pane: its half's other cell expands, or, when it was a whole
  half, the other half's division becomes the primary split (a pair). The focus stays on its tab, or
  moves to the pane that took the space.
- The app keeps **one layout per window** (a pair, or three or four panes). While a tab outside the
  layout is selected the whole layout is *parked* (`split_parked`), exactly like Chrome showing the
  other tab alone; selecting any member un-parks it with that pane focused. New tab, tab switch,
  workspace switch and Ctrl+Tab therefore never replace a pane. Closing either member of a pair
  dissolves the pair; closing a pane of a larger layout removes that pane (its sibling expands),
  shown or parked.
- Opening a pair moves the right tab directly after the left one in the tab model, so both strips
  show the halves together.
- A small in-memory ring of recently closed tabs (http(s) only, never private tabs, 8 entries,
  never written to disk) feeds the empty side's "Recently closed" list.
- Crash recovery saves the shown pair as `split <first> <second> <focused-first 0|1> <ratio>`; a
  stacked pair appends a fifth field `v`. A side-by-side pair keeps the four-field line older builds
  read, and every older file (no fifth field) restores side by side; any other fifth field drops the
  pair (`session.rs`, `stacked_split_round_trips_and_older_split_lines_restore_side_by_side`).
  A layout of three or four panes appends four more fields, `split L R F ratio <h|v> <trail1|->
  <trail2|-> <cross> <focused-trail 0|1>`; a pair still writes the old line. Older builds see a
  fifth field followed by more fields and drop the layout (the tabs restore alone); this build
  drops any line with another field count or an invalid value. A pane whose tab is not saved
  (private, blank) is taken out of the saved layout first (`without`), as a pair that loses a tab
  was never saved (`layouts_of_three_and_four_panes_round_trip_and_bad_lines_drop_the_layout`).

### 2.2 Layout and divider (native window + overlay)

| Item | Value | Token / source |
|---|---|---|
| Page gap (divider width and hit width) | 8 DIP | `--bw-space-2`, reference-spec 8px hit |
| Page corner radius | 9 DIP, both panes, all four corners | `--bw-r-tab` (same as a single page) |
| Active pane edge | 2 DIP accent ring inside the rounded edge | `--bw-accent` (`omnibox_focus_border`) |
| Inactive pane edge | 1 DIP hairline | `--bw-border` (`omnibox_border`) |
| Grip | 4 x 32 DIP pill centred in the gap; lying flat (32 x 4) in a stacked pair's gap | `--bw-r-pill`; idle text 22% (`--bw-pill-edge`), hover text 60%, drag accent |
| Minimum pane width | 240 DIP; below 2 x 240 + 8 the split collapses to the focused pane | |
| Minimum pane height (stacked) | 160 DIP (`MIN_PANE_HEIGHT_DIP`); below 2 x 160 + 8 the same collapse | |
| Ratio | 20-80% of the split axis, stored per pair, saved with crash recovery | |
| Three or four panes | the primary gap runs the full length; each divided half has its own cross gap at the shared cross ratio (20-80%); every pane keeps 240 x 160 DIP | `split_view::geometry`, `layout::split_panes` |
| Collapse (too small) | the page cannot hold two panes along the primary axis: only the focused pane shows (as a pair); a divided half cannot hold two cells across: that half shows its focused cell, else its lead, and the other is hidden. Nothing leaves the layout; everything returns when the window grows | `small_pages_collapse_without_dropping_tabs` |
| Grips | one 4 x 32 pill per divider; a divided layout's primary gap gets one pill on each side of the crossing, and the crossing itself a 6 DIP square (drag moves both dividers, `IDC_SIZEALL`) | `paint_split_grips` |
| Stacked layout | two full-width pages one above the other, the 8 DIP gap between them; every corner, edge and accent rule as side by side | `layout::split_pane_layout_stacked` |
| Divider cursor | `IDC_SIZEWE` side by side, `IDC_SIZENS` stacked | |

The edges, gap and grip are drawn by the existing layered chrome overlay, which sits above the
WebView2 child windows and is transparent to input, so nothing covers or delays page input. The
gap itself is the main window's client area, so hit-testing it needs no extra window. A page that
swallows its own clicks still reports focus through WebView2 `GotFocus`, which focuses its pane.

Divider input: drag resizes along the split axis (x side by side, y stacked; bounds are applied at
most once per animation frame); a click on the grip without moving opens the divider menu; a
right-click anywhere on the divider opens the same menu; double-click restores 50/50 (kept from
Bowser 1.x and documented; Chrome swaps instead). With three or four panes every gap is a divider
(`split_view::Divider`: `Primary`, `Cross(half)`, or `Both` at the crossing). A cross gap moves the
shared cross ratio, so in a 2 x 2 both cross gaps move together; the crossing moves both ratios at
once. Hover lights only the held divider's grips; double-click resets the divider under the
pointer (both at the crossing).

Divider menu (HTML popup menu, `menu::show`), anchored right of a side-by-side gap or below a
stacked one: Separate views (`unsplit`), Swap sides (`swap`), Stack top and bottom
(`split-stacked`), separator, Close left tab (`split-left`), Close right tab (`split-right`). A
stacked pair reads Swap top and bottom, Show side by side (`split`), Close top tab (`split-top`),
Close bottom tab (`split-bottom`). The tab menu's Swap sides follows the same wording.

With three or four panes the menu is generalised: Separate views, the divider's Swap (the primary
divider, the crossing and the toolbar button: Swap left and right / Swap top and bottom, mirroring
the halves; a cross divider: swap the two cells of that half), separator, then Close *place* tab
for each pane beside that divider (every pane for the primary divider, the crossing and the
toolbar; the half's two cells for a cross divider), named and drawn by place: Close top-left tab
(`split-top-left`), Close right tab (`split-right`) and so on. Turning (Stack top and bottom) stays a
pair-only entry.

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

**Drop-zone preview.** While a tab is dragged out of the HTML strip (below it) or out of the
vertical sidebar (right of it) over the page, the same host shows a placeholder card over the target
half, left, right, top or bottom (`split_view::drop_rect`: the half a new 50/50 pair gives that
side, the 8 DIP gap taken out of the middle): accent-tinted surface, dashed accent edge, the
half's glyph (`split-left`, `split-right`, `split-top`, `split-bottom`) in an accent badge, the
label (Body strong: "Open on the left", "Open on the right", "Open at the top", "Open at the
bottom") and the tab title (Caption). A half shorter than 220 px lays the card out in one row. Rust
raises its container above the page's WebView2 window while it shows (and falls back to the
overlay tint of the same half when the host is not ready).

*Which half (`split_view::drop_zone`).* The nearest page edge wins, each distance measured as a
fraction of the page's width (left, right) or height (top, bottom), so the page is cut along its
diagonals into four triangles; ties go to left/right. Near a diagonal the zone shown stays until
another edge is 4% nearer, and in the central box (the middle 20% of each axis) the zone shown
stays, or with none the left/right half the pointer is on. Every point of the page therefore has a
zone; off the page (over the toolbar, the sidebar, a docked DevTools pane or a side panel, which
`split_viewport` excludes) there is none, and releasing there is a plain reorder or nothing.

*Corners (`split_view::drop_zone_cornered`, October 2026).* Over a shown split, the pointer within
25% of a corner along both axes (`CORNER_PERMILLE`; once shown, the corner holds until 30%,
`CORNER_HYSTERESIS_PERMILLE`) is that corner's zone; everywhere else the edge rule above applies
unchanged. Corners are not offered on an unsplit page (one drop adds one pane, and a quarter would
need a second, empty pane, so a corner there is simply the nearest-edge half as before) nor while
dragging a half of a plain pair (its drags keep re-arranging the pair along the edges)
(`corners_allowed`). The drop map is in `quad-split/zone-map.png`.

*What a corner drop does (`split_view::plan_layout_drop`).* Decided on the page's quadrants: the
dragged tab takes that quadrant. If the pane there is a whole half, that half is divided and its tab
keeps the other cell, so a pair becomes a T (3 panes) and a T a 2 x 2 (4 panes); dropping one tab
adds at most one pane. If the pane there is already a quarter (a 2 x 2, or the divided half of a T),
the dragged tab **replaces** it and that tab leaves the layout as a normal tab (an empty pane
closes instead; a corner over the empty side fills it). A pane of the layout dragged onto another
quarter pane **trades places** with it (nothing leaves); onto its own quarter, nothing happens; any
other drop first takes it out of its pane (its sibling expands) and then drops it like an outside
tab. On three or four panes the **edges** mean "this whole half": the dragged tab takes it, the
tabs only in that half leave (normal tabs), the other half stays. The ratios stay; the dragged tab
is focused. The card covers exactly the rectangle the dragged tab will get in the planned layout
(`drop_target_rect`), and its words name that place (`drop_target_zone`): Open at the top left,
Open on the right, ...

| Before | Drop | After |
|---|---|---|
| no split, A shown | any corner | the nearest edge's half (pair), as before |
| pair A \| B | top-left / bottom-left | [X / A \| B] or [A / X \| B] (3 panes) |
| pair A \| B | top-right / bottom-right | [A \| X / B] or [A \| B / X] |
| stacked A / B | a corner | that row divided: [X A / B], [A X / B], [A / X B], [A / B X] |
| T [A / C \| B] | top-right / bottom-right | 2 x 2 (4 panes) |
| T [A / C \| B] | top-left | X replaces A; A becomes a normal tab |
| 2 x 2 | a corner | X replaces that pane's tab |
| 2 x 2, pane D dragged | another corner | D and that pane trade places |
| 3-4 panes | an edge | X takes that whole half; the tabs only there leave |

*What a drop does (`split_view::plan_drop`).* One rule: the dragged tab takes the zone's half and
its partner the other half, in the zone's orientation. No pair: the partner is the shown tab
(dragging the shown tab itself gives it an empty side). A shown pair and one of its halves: the
partner is the other half, so dragging a half re-arranges the pair (swap, or turn it stacked or
side by side); dropping a half where it already is shows no zone. A shown pair and an outside tab:
the partner is the focused half (unless that is the empty side, then the other half) and the
remaining half leaves the pair, staying open as a normal tab, or closing when it was the empty
side. The ratio follows the partner: kept, mirrored when the partner changes slot, 50/50 when the
orientation changes. Private tabs follow the same rules as before (nothing here pairs or lists a
tab the existing rules did not).

**Tab strip and vertical sidebar.** A `pair` message (Rust to tabs and toolbar views) names the
two tabs and the focused side. `ui/src/split-logic.ts` places the right tab directly after the left
one (same group and pin state for display) and the two tabs render as one joined capsule:

| Item | Spec |
|---|---|
| Halves | no gap between them; outer corners `--bw-r-tab`, inner corners 3px; 1px `--bw-hairline` seam |
| Shown pair | both halves get the island fill; the focused half keeps the active styling |
| Width | shown pair: each half flex 1.25 (pair 2.5, the widest unit); parked pair: each half 0.7 (pair 1.4, narrower than the active tab's 1.5) |
| Sidebar | the two rows join vertically into one capsule (inner corners 3px, seam hairline) |
| A11y | each half stays a `role="tab"`; its description is "Left side of split view" / "Right side of split view", or "Top side ..." / "Bottom side ..." for a stacked pair (`pair` carries `o`), plus ", focused" on the focused half of a shown pair |
| Stacked pair | the same joined capsule (the strip shows the pair, not its geometry) |
| Three or four panes | one joined capsule of every pane's tab in tree order (`pairOrder`); `data-split` `l` (first), `m` (middle: inner corners both sides) and `r` (last); shown panes flex 1 each, parked 0.7; descriptions name the place: "Top-left pane of split view", "Right pane of split view", ", focused" |

**Toolbar button.** `aria-pressed` follows the shown split; the glyph shows the focused side
(`split-left` / `split-right`, or `split-top` / `split-bottom` when stacked, otherwise `split`). In
a layout of three or four panes it shows the focused pane's place: a quarter (`split-top-left`,
`split-top-right`, `split-bottom-left`, `split-bottom-right`, new glyphs) or a half.
Click: not split, open a split with an empty right side; split, open the split menu (as the
divider menu).

**Empty side, stacked.** The picker covers whichever pane is empty (top or bottom) and its heading
names it: "Choose a tab for the top side" / "... bottom side".

**Empty pane in a larger layout.** There is still one picker host per window: it covers the first
empty pane in tree order ("Choose a tab for the top-left pane" for a quarter); a second empty pane,
which only arises in odd sequences, keeps the overlay's opaque fill until the first is filled.

### 2.4 Entry points

| Entry | Result |
|---|---|
| Toolbar split button / Ctrl+Shift+\ | Not split: active tab left, empty side right. Split shown: button opens the split menu; the shortcut separates views |
| Tab menu on an inactive tab: "New split view with current tab" | Active tab left, this tab right, right focused |
| Tab menu on the active tab: "Add tab to new split view" | Active tab left, empty side right |
| Tab menu on a tab of the pair | "Separate views", "Swap sides" |
| Link context menu: "Open link in split view" | New tab with the link beside the current tab |
| Drag a tab below the strip (or right of the vertical sidebar) onto the left, right, top or bottom half of the page | Dragged tab on that half, its partner on the other (see Drop-zone preview); top and bottom make a stacked pair |
| Divider grip / right-click / toolbar split menu: Stack top and bottom, Show side by side | Turns the shown pair |
| Divider grip / right-click | Divider menu |

Keyboard: Ctrl+Shift+\ toggle, Ctrl+Shift+Left/Right focus a side (the first / second pane in
either orientation), Ctrl+Shift+Up/Down focus the top / bottom pane (bound only while a stacked
split is shown, so pages keep the chords otherwise), Ctrl+Shift+W close the focused side's tab.
With three or four panes the arrows move to the geometric neighbour (`SplitView::neighbour`; from a
whole half the top or left neighbour wins; nothing at the page's edge), Up/Down are bound whenever
panes sit above one another (`has_rows`), and Ctrl+Shift+W closes the focused pane (its sibling
expands).

### 2.5 Toolbar follows the focused side

The focused side's tab is the app's active tab, so back/forward/reload, the address text,
security chip, zoom chip, private chip, reader state and the window title all follow it. Focus moves
on: a click in either page (WebView2 `GotFocus`), a click on the gap's side of a pane, the picker,
Ctrl+Shift+Left/Right, or selecting either half in the strip.

### 2.6 Motion (transform and opacity only; nothing waits for it)

- Empty side: its column enters with `.bw-enter.is-scale` (opacity 0 to 1, .98 to 1, 167ms out-ease).
- Drop-zone card: `.bw-enter.is-scale` when it appears and again when the zone changes (the host
  moves to the new half); it leaves at once (no exit animation, so a drop is never visually delayed).
- Turning a pair (menu or drop) re-lays the panes in one step; nothing animates the native pages.
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
  the tab menu via Shift+F10 on a focused tab, the picker fully). A stacked pair, which a drag
  creates, is also reachable from the split menu (Stack top and bottom).
- The drop card is a `role="status"` region named after its label and the tab title ("Open at the
  top: Docs").

### 2.8 Dropped or deferred

- Inactive-pane dimming (would need a full-pane alpha wash in the overlay on every repaint and
  lowers the inactive page's legibility; the edge colour carries the state instead).
- Chrome's per-page origin chip with its own menu (the overlay is input-transparent; the divider
  grip and the toolbar button carry the menu instead).
- More than one layout per window, more than four panes, three columns or rows, and independent
  (unaligned) cross dividers in a 2 x 2; the "pin split button" setting, and the setting that
  disables drag-to-split. (Stacked split and up to four panes shipped; see section 3.)
- Dropping relative to one pane (Chrome-style per-pane edges): the zones are the page's four
  halves and four corners.
- The native (GDI) strip's own drag stays pair-only (edges only, no corners); the tab menu's "New
  split view with current tab" and the link menu still make a pair (replacing a larger layout; its
  other tabs stay open as normal tabs).
- Keyboard resizing of the dividers; dragging a 2 x 2's cross gaps apart.
- Keyboard resizing of the divider.
- Persisting a parked pair across crash recovery (only the shown pair is saved, as before).
- The native split button and native drop tint stay only for the native (GDI) chrome; in HTML mode
  the native split button paint is skipped and the HTML surfaces replace the drop tint.

## 3. Changelog

- **Stacked split and drag anywhere (October 2026).** A tab can be dragged onto any edge of the
  page, left, right, top or bottom, to split that way; top and bottom make a stacked pair (two
  full-width pages, 160 DIP minimum height, horizontal gap and grip, `IDC_SIZENS`). Drags out of
  the vertical sidebar now split too. The zone rule, the drop rule and the four-edge card are in
  section 2.3; the model (`Orientation`, first/second slots) in 2.1; Stack top and bottom / Show
  side by side in the split menus; Ctrl+Shift+Up/Down while stacked; `pair` gains `o`,
  `splitpick` / `splitdrop` gain sides `t` and `b` (docs/html-chrome-protocol.md, "Split view");
  crash recovery appends `v` for a stacked pair and reads every older line as side by side. New
  glyphs `split-stacked`, `split-top`, `split-bottom`. Layout and zone logic are unit-tested; the
  native drag, overlay and WebView2 placement still need a supervised real-app pass.
- **Four panes from the corners (October 2026).** Dragging a tab onto a corner of a shown split
  divides the pane there so the tab lands in that quarter; repeated corner drops build a T (three
  panes) and a 2 x 2 (four), and a corner of a 2 x 2 replaces that pane (a pane of the layout trades
  places instead). Model: `SplitView.trail` / `cross_ratio`, `Pane::LeftTrail` / `RightTrail`,
  quadrant maps (`cells`, `from_cells`), `without`, `neighbour` (2.1); corner zones with
  hysteresis and the drop table (2.3); every gap a divider with its own grip, the crossing moves
  both (2.2); generalised divider / toolbar menus; joined capsule of N tabs with place names;
  `pair` gains `panes` and `fp` for three or four panes, `splitpick` / `splitdrop` take corner
  sides (docs/html-chrome-protocol.md); crash recovery appends four fields for a larger layout and
  still writes and reads the old pair line. New glyphs `split-top-left`, `split-top-right`,
  `split-bottom-left`, `split-bottom-right`. Model, zones, geometry (96 and 192 DPI), collapse,
  session and wire are unit-tested; screenshots and diagrams are in
  `C:\Users\charlie\.t3\shots-tmp\quad-split\`. The native drag, overlay and WebView2 placement of
  three and four panes still need a supervised real-app pass.
