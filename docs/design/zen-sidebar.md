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

## The sidebar

From the top: the pinned tabs as a grid of large tiles, the space header (workspace name, a rule and
**Clear**, which closes every tab that is not pinned), **New tab**, then the tabs. Rows and tiles reorder by
dragging. A row dragged above the grid (or above the first row when nothing is pinned) pins its tab; a tile
dragged below the grid unpins it (`sidebar-logic.ts` `pinFlipIntent`). The target is outlined while the
tab is held there.

## Not verified here

Built and type-checked for Windows and covered by the UI tests; the window itself was not run.
