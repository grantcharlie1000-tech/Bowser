Window appearance and Panels (thread T9): headless before/after screenshots of the settings page's Appearance area.

  node benchmarks/results/t9/render.mjs benchmarks/results/t9 after
  node benchmarks/results/t9/render.mjs benchmarks/results/t9 before

"before" is origin/t6/layout's committed ui/dist, "after" is this branch's. Headless Edge, fake-host.js
standing in for the WebView2 bridge. PNGs: <before|after>-<scene>-<dark|light>-<100|150>.png.
  appearance          the core view: unchanged (no new core rows)
  appearance-more     More options opened: the Window card gains five rows and a Reset, a new Panels
                      card has three rows and a Reset
  appearance-more-on  (after only) every new option switched to its non-default value; the sidebar width
                      and UI zoom rows show minus/plus buttons instead of sliders
