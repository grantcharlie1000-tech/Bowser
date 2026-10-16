Start page: headless before/after screenshots and timings (docs/design/start-page.md).

  node benchmarks/results/sp/harness/render.mjs benchmarks/results/sp <before ui/dist folder> [name filter regex]

Renders the split host (the start page) and the settings page from a ui/dist folder in headless Edge through
the Chrome DevTools Protocol (no window on the desktop), with host.js standing in for the WebView2 bridge and
the messages Rust writes (ui/test/fixtures). "before" is origin/main's ui/dist (a blank new tab, and the
settings page without the Start page section); "after" is this branch's.

PNGs: <before|after>-<scene>-<dark|light>[@150].png, 100% and 150% (device scale 1.5).
  newtab            a blank new tab (before)
  start             groups, dials, all six widgets      start-nowidgets / start-empty / start-guest
  start-images      two dials with a chosen image       start-large (titles when needed) / start-small (tiny tiles)
  start-custom      custom widget colour, tall, 3 cols  start-solid (solid, 4 columns)
  start-folder      inside a sub-folder
  start-dialog / -menu / -config   the add-site dialog, the group menu, the bookmarks widget's settings strip
  settings-top      the settings page top (before: 10 sections, after: 11)
  settings-startpage   the Start page section

timing.txt: message-to-page-built times in headless Edge (20 cold page loads each). The page runs in its own
renderer process, so this does not block the browser's UI thread; the figures are noisy because other work was
running on the machine, and the "start-page.js fetch" part is a localhost HTTP fetch, where WebView2 serves
the file from memory. rust-cost.txt: the UI-thread side.
