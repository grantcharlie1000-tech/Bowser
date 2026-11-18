# Zen-style vertical tabs

With the tab bar on a side (the Vertical left / Vertical right presets) and the address bar shown, Bowser
lays the window out the way Zen does. Nothing else in Settings places the bars: the **Bar positions** row
is gone and the layout presets decide every edge.

## Layout

`chrome::layout::compute_zen_in_frame` builds one column on the docked edge, the sidebar's width:

1. A title row with drag space and the window buttons at its right end (`Layout::caption_area`). The frame
   hit test reads the same place through `frame::frame_hit_test_captions_at`.
2. The toolbar, two rows (`Layout::toolbar_in_sidebar`): back, forward, reload and the action buttons with
   the main menu, then the address field across the column. The toolbar page switches to the stacked CSS
   by the host's height (`toolbar.css`, "Zen sidebar"); no protocol change.
3. The tabs host under them, to the bottom edge.

The page takes the rest of the window at full height. There is no left rail any more.

## Compact mode (auto-retracting sidebar)

**Pin sidebar** off (`sidebar_collapsed=true`, also Ctrl+S and the sidebar's pin button) is compact mode:
`auto_hide::with_compact_sidebar` adds the tabs to the hidden bars, so the whole column (window buttons,
toolbar and tabs) leaves the layout and the page fills the window. Resting the pointer on the docked edge
slides the column back over the page without resizing it; it hides again 500 ms after the pointer leaves,
and stays while the address field has focus or a menu is open. This is the T11 auto-hide machinery
(`docs/design/t11-auto-hide.md`) with the toolbar in the sidebar zone (`WindowChrome::toolbar_zone`).

## The floating card

A hidden column comes back as one floating card over the page, the way Zen's compact mode looks, in
Bowser's own colours. Unpinned is the default for a new profile (`sidebar_collapsed=true`), so vertical
tabs stay out of the way until the pointer reaches the docked edge; a profile that saved a pinned
sidebar keeps it, and Ctrl+S, the sidebar's pin button or Settings > Layout > Pin vertical tabs switch
between the two.

- Geometry (`chrome::layout::float_zen_column`, applied by `compute_arranged_auto_hide` whenever the Zen
  column is out of the layout): the card (`Layout::float_island`) is the column inset by
  `ZEN_FLOAT_MARGIN` (4 DIP) on every side, so a strip of page shows all round it. Everything inside
  stays the `ISLAND_INSET` (8 DIP) from the column's sides it already had, which leaves it 4 DIP inside
  the card's `ZEN_FLOAT_RADIUS` (10 DIP) corners: a square host corner that far in is always inside the
  curve. The title row, the window buttons and the toolbar move down by the same 8 DIP, the window
  buttons end where the toolbar does instead of at the window's edge, and the tabs give up that height
  at their top. The page never moves.
- Paint (`ChromeSurface::paint_float_island`, on the click-through layered overlay that already sits
  above every child window): a two-layer shadow (a wide key shadow dropped 6 DIP, a tight ambient
  edge), the solid window colour inside anti-aliased rounded corners, and a hairline edge 16% of the way
  to the text colour. The toolbar and tabs hosts are cleared out of it so their HTML views show; their
  backdrop is the same solid colour, so the card reads as one surface. The sidebar page drops its own
  island (`.is-float`, from `float:true` on the `layout` message).
- Motion: the card, the window buttons and both hosts slide in from the docked edge together over
  220 ms (ease-out in, ease-in out; `auto_hide::SLIDE_MS`), with the painted layout moved by the same
  offset (`Layout::shift_float_column`). The 150 ms reveal dwell and 500 ms hide delay are unchanged.
- Clicks: the overlay lets clicks through, so a plain child window under the card (the catcher,
  `win32/window/auto_hide.rs`, cut to the card's rounded corners) takes presses on the card's own
  surface instead of the page under it: the title row drags and double-click maximises like a caption,
  and the window buttons fall through to the frame's hit test, which tests them where the card has
  them (`WindowChrome::floating_caption`).
- It goes when the pointer leaves it, even with Bowser in the background; it stays while the address
  field has focus, a menu or popup is open or a tab is being dragged.

## The sidebar

From the top: the pinned tabs as a grid of large tiles, the space header (workspace name, a rule and
**Clear**, which closes every tab that is not pinned), **New tab**, then the tabs. Rows and tiles reorder by
dragging. A row dragged above the grid (or above the first row when nothing is pinned) pins its tab; a tile
dragged below the grid unpins it (`sidebar-logic.ts` `pinFlipIntent`). The target is outlined while the
tab is held there.

## Not verified here

Built and type-checked for Windows and covered by the UI tests; the window itself was not run. The
floating card's Rust tests (`assert_floats`, `only_a_hidden_zen_column_floats_and_the_card_slides_with_it`,
`a_floating_zen_column_paints_one_card_with_a_soft_shadow_and_clear_hosts`) compile for Windows but were
not run (no Windows or working Wine where they were written).
