Two-level tab stacks (thread T10b): headless before/after screenshots.

  node benchmarks/results/t10b/render.mjs benchmarks/results/t10b before
  node benchmarks/results/t10b/render.mjs benchmarks/results/t10b after
  BOWSER_DUMP_TWO_LEVEL=<dir> cargo test --locked --lib dump_two_level -- --ignored

render.mjs draws the HTML strip and settings page from a ui/dist folder in headless Edge (no window on
the desktop), fake-host.js standing in for the WebView2 bridge. "before" is the branch base's committed
ui/dist and fixtures (T10 + T6), "after" this tree's. The strip host is as tall as Rust makes it: 28 px
for one row, 58 with the second row, 52 with the compact second row.

PNGs: <before|after>-<scene>-<dark|light>-<100|150>.png (device scale 1 and 1.5).
  strip                        one tab row: a folded "Docs" stack (Compact), unchanged by this branch
  strip-two-level              Two-level: the stack is its open ball, its three tabs fill the second row
  strip-two-level-compact      the same with "Compact second row"
  settings-stacks              Settings > Tab stacks: the fifth choice, Two-level
  settings-compact-row         behind More options: "Compact second row", off-limits until Two-level

gdi-<before|after|after-compact>-<dark|light>-<100|150>.png: the native GDI fallback strip at 96 and
144 DPI (the toolbar, the tab row and, after, the second row), from the dump test above.
