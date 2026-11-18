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
brings the column back docked, in its pinned place: it pushes the page aside (the page is laid out again
beside it) rather than floating over it. It hides again 500 ms after the pointer leaves, and stays while
the address field has focus or a menu is open. This is the T11 auto-hide machinery
(`docs/design/t11-auto-hide.md`) with the toolbar in the sidebar zone (`WindowChrome::toolbar_zone`).

## The floating card

> Superseded for reveals: a revealed column now docks and pushes the page (see Compact mode above), so
> the card below is no longer shown. The top-edge strip with the window buttons stays, and pushes the
> page down while it is out (no shadow). The geometry is kept because the hidden layout still computes it.

A hidden column comes back as one floating card over the page, the way Zen's compact mode looks, in
Bowser's own colours. Unpinned is the default for a new profile (`sidebar_collapsed=true`), so vertical
tabs stay out of the way until the pointer reaches the docked edge; a profile that saved a pinned
sidebar keeps it, and Ctrl+S, the sidebar's pin button or Settings > Layout > Pin vertical tabs switch
between the two.

- Geometry (`chrome::layout::float_zen_column`, applied by `compute_arranged_auto_hide` whenever the Zen
  column is out of the layout): the card (`Layout::float_island`) keeps `ZEN_FLOAT_MARGIN` (6 DIP) of
  page between itself and the page's top, bottom and outer edges (inside the window's resize rim, so the
  gap is always page, never window frame) and on its inner side. Inside it, `ZEN_FLOAT_PAD` (6 DIP) from
  its edge (inside its `ZEN_FLOAT_RADIUS` 10 DIP corners): the address field across the top row, the
  tabs, and the navigation row (back, forward, reload, the actions and the main menu) along the bottom.
  The toolbar host spans the whole content and its page puts the address row at its top and the
  navigation row at its bottom when it is that tall (`toolbar.css`, "Floating zen sidebar"); the tabs
  host lies over its middle (raised after it). The main menu opens from the button's top, so it flips
  above the bottom row. The page never moves.
- The window buttons and the drag space leave the card for a strip along the window's top edge
  (`Layout::float_band`, with `title_bar` and `caption_area` in it): a second reveal, the auto-hide top
  zone, from its own sensor on the very top edge. It slides down on its own (`Layout::shift_float_band`)
  and hides when the pointer leaves it; the card does not come with it, nor it with the card. A hidden
  bookmarks bar rides with it, hanging under it.
- Paint (on the click-through layered overlay that already sits above every child window): the card
  (`ChromeSurface::paint_float_island`) is a two-layer shadow (a wide key shadow dropped 6 DIP, a tight
  ambient edge), the solid window colour inside anti-aliased rounded corners, and a hairline edge 16% of
  the way to the text colour; the strip (`paint_float_band`) is the solid window colour with that
  hairline at its foot and a soft shadow on the page below, the window buttons drawn on it. The HTML
  hosts are cleared out of the card; their backdrop is the same solid colour, so it reads as one surface.
  The sidebar page drops its own island (`.is-float`, from `float:true` on the `layout` message).
- Motion: the card and both hosts slide in from the docked edge together over 220 ms (ease-out in,
  ease-in out; `auto_hide::SLIDE_MS`), the strip from the top the same way. The 150 ms reveal dwell and
  500 ms hide delay are unchanged.
- Clicks: the overlay lets clicks through, so a plain child window under the card and one under the strip
  (the catchers, `win32/window/auto_hide.rs`; the card's cut to its rounded corners) take presses on their
  own surface instead of the page under them: the strip drags and double-click maximises like a caption,
  and the window buttons fall through to the frame's hit test, which tests them where the strip has them
  (`WindowChrome::floating_caption`).
- Each goes when the pointer leaves it, even with Bowser in the background; they stay while the address
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
