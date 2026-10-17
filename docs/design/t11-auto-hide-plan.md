# T11 Auto-hide: pointer detection plan (for approval before building)

Status: plan only, nothing built. Baseline `origin/vivaldi/wave1-review`. T11 runs in wave 3, after T6 Layout merges.

## The problem

Tab pages are windowed WebView2 controllers (`CreateCoreWebView2Controller(parent, ...)` in
`src/engine/webview2.rs`). Their child HWNDs belong to the WebView2 browser process, so while the pointer
is over a page, Bowser's window gets no `WM_MOUSEMOVE`. When the bars are hidden and the page fills the
window, Bowser cannot see the pointer reach the edge on its own.

## Options

| Option | Latency | Reliability | Fullscreen | Cost to input, focus, launch | Verdict |
| --- | --- | --- | --- | --- | --- |
| **A. Native edge sensors**: Bowser's own pixels at the reveal edge. Windowed: the existing native top inset and resize border. Maximised/fullscreen: a thin `WS_CHILD` sensor window placed above the content in z-order. | Immediate (`WM_MOUSEMOVE` on the sensor) | High where the screen edge stops the cursor; a fast pass over a shared monitor edge can miss | Good: the cursor stops at y=0 | None. No hook, no polling, no page script. The sensor never takes focus (`WS_EX_NOACTIVATE`, `MA_NOACTIVATE`). It costs the page one strip of pixels, and only while the bars are hidden. | **Recommended** |
| B. `GetCursorPos` poll on a timer while hidden | Up to one tick (50 ms) | High | Good | 20 wakeups a second the whole time the bars are hidden (battery). Input is not delayed. | Used only while the bars are shown, to time the hide |
| C. `WH_MOUSE_LL` low-level hook | Immediate | High | Good | Global: every mouse event on the system passes through Bowser. A stalled thread makes the whole desktop's mouse lag until Windows drops the hook. Too much for an edge test. | Rejected for T11 (T12 needs it; see that plan) |
| D. Composition-controller hosting (`SendMouseInput`) | Immediate | Total | Good | A rewrite of tab hosting (DirectComposition, cursor, IME, drag and drop, accessibility). Adds risk to every input path and to cold launch. | Rejected |
| E. Page JS `mousemove` + `postMessage` | IPC hop, 5-20 ms | Fails where scripts don't run (PDF viewer, error pages, `edge://` pages, blank tabs) | Partial | Turns on the page message channel, which is off today for security (only the password login message uses it). Pages can spoof or block it. | Rejected |
| F. `TrackMouseEvent` / `WM_MOUSELEAVE` on the parent | n/a | Fires when the pointer *enters* the page, not when it reaches an edge | n/a | None | Not usable for reveal |

## Recommendation: A, native edge sensors. Hide is timed by a short poll while the bars are shown.

- **Windowed (restored):** the native 6 DIP top inset and the resize border stay outside the page
  (`docs/html-chrome-protocol.md`: the inset stays native and draggable). Bowser's `WM_MOUSEMOVE` /
  `WM_NCMOUSEMOVE` on that band starts the reveal. No new window. The window can still be dragged and
  resized as now.
- **Maximised and fullscreen:** one sensor `WS_CHILD` per edge that has hidden bars, raised above the
  content controller (`SetWindowPos(HWND_TOP, SWP_NOACTIVATE)`), invisible (no paint, arrow cursor).
  Thickness is 1 physical pixel when the screen edge stops the cursor. It is 4 DIP when
  `MonitorFromPoint` finds another monitor past that edge, so a crossing pointer still lands on it. DPI
  comes from `GetDpiForWindow`, and the sensor is re-placed on `WM_DPICHANGED` and on every layout pass.
- **Reveal delay:** the pointer must stay on the sensor for 150 ms (one `SetTimer`), so passing
  through on the way to another monitor or the taskbar does not open the bars. Keyboard reveal has no
  delay.
- **Hide:** while the bars are shown, a 100 ms `GetCursorPos` check (only then) hides them 500 ms after
  the pointer leaves the bars' rectangle plus an 8 DIP margin. They never hide while the address bar has
  focus, a menu, popup or flyout from them is open, a tab is being dragged, or the window is inactive.
- **Keyboard:** Ctrl+L / Alt+D / F6, Ctrl+T, Ctrl+Tab and Quick Commands reveal the bars they need and
  keep them shown until focus returns to the page. Nothing becomes unreachable by keyboard.
- **Motion:** a slide using the existing motion tokens. With `prefers-reduced-motion` they appear
  instantly. Revealing never moves focus away from the page.

## Overlay, not resize (how `layout.rs` reflow is avoided)

The revealed bars **float over** the page. The page is not resized, so showing or hiding the bars
never relays out the page, fires `resize` events or shifts its content.

- `chrome/layout.rs` gets one input, `AutoHide { hidden: EdgeSet, shown: EdgeSet }`. With bars hidden,
  `viewport` covers what those bars used to take. A new `overlay` rect per edge gives where the bars
  draw when shown. With auto-hide off, the existing results are byte-for-byte the same; a test pins
  that.
- The HTML chrome hosts (toolbar, tab strip, bookmarks bar, panel host) are already child HWNDs that
  Rust places. When shown they are placed at `overlay` and raised above the content controller. Child
  HWNDs cannot be see-through over a sibling page, so overlaid bars use the existing opaque `solid:true`
  palette.
- The side panel and vertical tabs overlay the same way from the left or right edge.
- **GDI fallback:** native chrome is painted by the main window, which cannot paint over a child page.
  There, revealing pushes the page instead (a resize). The fallback is rare, and the T6 rule (native,
  HTML and GDI geometry agree) is kept by computing both from the same `layout.rs` result.

## Bars, edges, fullscreen and toggles

- Bars that can hide: tab bar (horizontal or vertical), address bar, bookmarks bar (when it is shown),
  side panel, status bar (T6 adds it; it reveals from its own edge). Top bars reveal together from the
  top edge, side bars from their side, the status bar from the bottom.
- **Fullscreen does not exist in Bowser today**: no F11, and no `ContainsFullScreenElementChanged`
  handler. T11 would add both: F11 window fullscreen (borderless, monitor-sized, restored on exit), and
  page element fullscreen, where the page asks (video). In fullscreen, auto-hide is always on when the
  setting below is on (the default). For page element fullscreen, all bars stay hidden and edges do not
  reveal; Esc exits, as in other browsers.
- **Toggle:** Ctrl+F11 (free today). The new registry command `toggle-auto-hide` ("Toggle auto-hide
  UI") appears in Quick Commands, the keyboard editor and chains automatically. The status bar button
  calls the same command once T6's status bar exists (T13 can then place it). The toggle flips the saved
  setting, as in Vivaldi.

## Settings (three rows, Appearance > Window)

1. **Auto-hide browser UI**: this is the existing `frameless` key, so the saved value and its tests
   are kept. The label changes from "Frameless window" to "Auto-hide toolbars and tabs". `notYet` is
   removed, and the row now acts live, not after a restart.
2. **Bars to hide**: new `auto_hide_bars` key, a check group (tabs, address bar, bookmarks bar, panel,
   status bar); default tabs + address bar + bookmarks bar. Disabled while row 1 is off.
3. **Always auto-hide in full screen**: new `auto_hide_fullscreen`, default on.

The reveal delay is fixed (150 ms reveal, 500 ms hide); there is no row for it unless you ask.
Each row gets the usual UI -> Rust save -> snapshot -> behaviour test (`layout.rs` geometry for each
bar set, sensor rects at 100/150 % with and without a neighbouring monitor, a reveal/hide state machine
on a fake clock, `frameless` round-trips under the new label).

## Cold launch and cost

- With auto-hide off, nothing new is created or run.
- With it on: the bars start hidden, which is less to show on the first frame. The sensors are made
  after first paint with one `CreateWindowEx` each, and there are no timers until the pointer touches an
  edge.
- No hook, no page script, and no message channel to pages.
