# Auto-hide toolbars and tabs (T11)

Settings > Appearance: **Auto-hide toolbars and tabs** (the old `frameless` key, now live) is the master switch.
Behind More options: **Bars to hide** (`auto_hide_bars`: tabs, address, bookmarks, panel; default the first three) and
**Always auto-hide in full screen** (`auto_hide_fullscreen`, default on). Ctrl+F11 flips the saved master switch
(`toggle-auto-hide`); F11 is full screen (`toggle-fullscreen`). Delays are fixed: 150 ms to reveal, 500 ms to hide.

## How it works

- Rules in `src/auto_hide.rs` (settings, which bars hide, the reveal/hide state machine on an explicit clock, sensor
  geometry); geometry in `chrome/layout.rs` (`compute_*_auto_hide`, `auto_hide_overlay`); the window side in
  `win32/window/auto_hide.rs`.
- Hidden bars leave the layout: the page is laid out over them and never resizes when a bar appears. A revealed bar is the
  same HTML view in its usual place, raised above the page windows (`ChromeHost::raise_to_top`), with the solid palette.
  With auto-hide off the layouts are field-for-field what they were (`auto_hide_off_changes_nothing`).
- Pointer detection (plan A): a thin child window per hidden bar's edge, on the page's edge (1 px at a screen edge with no
  monitor beyond, else 4 DIP), `WS_EX_NOACTIVATE` behaviour via `MA_NOACTIVATE`, made after first paint when first needed. No
  hook, no page script. While a reveal is pending or bars are shown a 50 ms tick checks the pointer; otherwise no timer runs.
- Bars stay while the address bar has focus, a menu or popup is open, a tab or split divider is dragged, or the window is behind.
  Ctrl+L, Ctrl+T and the other address-bar focus paths reveal the bars first.
- Horizontal layouts: the tab row and toolbar share the top edge, so choosing either hides both. Vertical: tabs = the sidebar
  (left edge), address bar = the toolbar (top edge). The side panel, when chosen, opens over the page instead of shrinking it.
  The bookmarks bar rides with the top chrome, or has its own edge strip when the top chrome stays.
- Not built (by decision): the status bar button and status bar hiding (T6 adds the bar; add a bit to `Bars` and a row to
  `BAR_TABLE`); mouse gestures (cancelled).

## Cost

Auto-hide off: nothing is created, no timer, no hook; `sync_sensors` and `after_layout` return at once. On: one
`CreateWindowEx` per sensor after first paint. Rules cost (debug build): about 190 ns for resolving the plan and one reveal,
7 us to parse and write both settings.

## Real-app checks (not run: headless only)

1. Switch on, hide tabs+address: page fills the window; the top edge reveals both after ~150 ms and hides ~500 ms after leaving.
2. Reveal does not reflow the page (watch a page with a resize listener).
3. Raised HTML bars sit above the page at 100% and 150%; vertical layout sidebar from the left edge.
4. Ctrl+L / Ctrl+T show the bar and it stays until focus returns to the page. Menus from the bar keep it up.
5. F11 covers the taskbar, restores size and maximised state; a video's full-screen button hides every bar; Esc ends it.
6. Maximised, and with a second monitor beside/above: a fast pass to the next monitor opens nothing.
7. New tab while a bar is up: the bar stays above the new page. Dragging the window by the top rim and resizing still work.
8. Ctrl+F11 flips the saved setting; relaunch keeps it.
