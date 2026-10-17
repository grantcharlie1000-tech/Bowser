Layout (thread T6): headless before/after screenshots of the settings page's Appearance area.

  node benchmarks/results/t6/render.mjs benchmarks/results/t6 after
  node benchmarks/results/t6/render.mjs benchmarks/results/t6 before

Renders the settings page from a ui/dist folder in headless Edge (no window on the desktop), with
fake-host.js standing in for the WebView2 bridge and the committed fixtures as the host's messages.
"before" is origin/vivaldi/settings-slim's committed ui/dist; "after" is this branch's.

PNGs: <before|after>-<scene>-<dark|light>-<100|150>.png, 100% and 150% (device scale 1.5).
  appearance        the Appearance area: before, the four layout tiles; after, the Layout block with the
                    six preset thumbnails (Simple, Classic, Vertical left, Vertical right, Bottom, Saved)
                    and the tab bar / address bar / panels edges
  appearance-more   the area's More options opened: the top style tiles, sidebar width, the show/hide
                    switches, the title row, the status bar, UI density, compact menus and the UI zoom
