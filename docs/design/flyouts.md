# Rail flyouts: History, Bookmarks, Customize

Status: design for `feat/flyout`. Replaces the docked left panel for these three rail panels when
the HTML chrome is on. Native chrome (`BOWSER_UI=native`, or the HTML panel controller failing)
keeps the docked `side_panel.rs` host with the GDI panels, unchanged.

User request: "Instead of opening a whole side bar, just have them open a hover menu."

## 1. Why the docked panel goes

The visual gate (`benchmarks/results/visual-gate/REPORT.md`, `ASTRA.md`) and the user's own
screenshot (`docs/design/user-report-empty-panel.png`: History opened on a loaded Wikipedia tab,
an empty dark rectangle with two stray `x`) show four defects that all come from *docking*:

| Defect | Cause in the docked design | How the flyout removes it |
|---|---|---|
| V1 panel hidden behind a loaded page | the page WebView must be re-bounded to make room; on a loaded tab it is not, so it covers the panel host | nothing is re-bounded: the flyout is a separate top-level window above the page |
| V5 stale host-painted "Panel"/"History" title over the toolbar | `side_panel.rs` paints its own GDI header and animates its width with a timer | no native host, no GDI paint |
| V10 stacked empty frames after open/close cycles | width animation + retired content + child HWND churn | one pre-created window, shown and hidden; content swapped in place |
| V23 two headers, two close buttons | host header (GDI) + HTML panel header | the HTML panel header is the only header |

So the fix is structural: no docked host, no page re-bounding, one window that only shows or hides.

## 2. Container decision

The flyout is an **owned, activating `WS_POPUP`** (`src/win32/flyout.rs`) that hosts the existing
`?view=panel` controller (`View::Panel`, `src/win32/html_panel.rs`, `ui/src/panel.ts`).

The menu popup host (`html_popup.rs`, `?view=popup`) was the first candidate. It is not used as
the container, for three reasons:

1. **It shows one page at a time.** Customize's "Switch profile" opens the profile menu, and every
   keyboard shortcut that opens a menu (main menu, site info, palette) must still work while a
   flyout is open. With one shared host the menu would have to replace the flyout's page.
2. **Its command whitelist is deliberately narrow** (`size`, `pick`, `dismiss`, `act`). The panel
   families (`hist`, `bm`, `cust`) are accepted only by the panel host. Widening the popup host's
   acceptance matrix is a security change for no gain.
3. **The panel runtime already exists.** `html_panel.rs` owns the snapshot and command plumbing for
   exactly these three panels (`chrome_host/panels/{history,bookmarks,customize}.rs`). Re-hosting
   its controller from the docked `WS_CHILD` into an owned popup keeps every snapshot, command,
   validation rule and test as is. No controller is added: the panel controller already existed
   (created lazily for the docked host); it is now created hidden and suspended with the other
   chrome controllers, exactly like the popup host.

What is shared with the popup host: the monitor DPI and work-area lookup
(`html_popup::monitor_data`), the reopen guard pattern (`DismissState`), the owned-popup focus
contract (remember the focus at open, give it back on close), suspension while hidden, and the
"page says it painted, then Rust shows the window" handshake.

## 3. Geometry

All values are DIP (CSS px) and scale with the DPI of the monitor the rail button is on.

```
  rail host                    flyout window (owned WS_POPUP, transparent outside the surface)
 |<-- R -->|
 |         |   4   +------------------------------------------------+ 16
 |  [cust] |<----->| History                                     [x] |<-->   <- 44px header,
 |         |   /|  |------------------------------------------------|        title + close
 |  [hist] |--< |  | [search history                              ] |        (.bw-icon-btn)
 |  caret  |   \|  |  Today 6                                       |
 |         |       |  W Web browser - Wikipedia    en.wikipedia 10:01|  <- internal scroll
 |         |       |  ...                                           |       (.bw-scroll)
 |         |       |------------------------------------------------|
 |         |       | Clear history   Last hour  Today  All time     |
 |         |       +------------------------------------------------+
 |         |                                                  24 (shadow room below)
```

| Quantity | Value |
|---|---|
| Surface left | rail host right edge + 4 (the caret tip reaches the rail; the surface covers the page frame edge) |
| Surface width | History, Bookmarks 360; Customize 380 |
| Width cap | never past the owner window: `owner client right - 8 - surface left` |
| Narrow window | owner client narrower than 500: the surface takes all of that width (full window minus rail) |
| Preferred height | History, Bookmarks 600; Customize 640 (fixed, so typing in search never resizes the window) |
| Vertical band | owner window rectangle intersected with the monitor work area, inset 8 top and bottom |
| Height | `min(preferred, band height)`: the list scrolls inside, the frame never does |
| Top | button centre − 22, so the 44 px header row is centred on the button; clamped into the band |
| Caret | centred on the button: `y = button centre − surface top`; hidden when it would fall within 18 px of the surface's top or bottom (the 8 px corners plus the caret's half-size) |
| Window rectangle | surface inflated by the shadow room: left 4 (the window starts at the rail edge, so it never covers a rail button), top 8, right 16, bottom 24 |
| Radius | `--bw-r-overlay` 8 px, drawn by CSS (no `SetWindowRgn`, which would clip the shadow) |
| Shadow | `--bw-sh-flyout` (0 8 16 / .14 and 0 0 2 / .12), drawn by CSS inside the shadow room, so it is never clipped. Earlier menus lost their shadow because the window region cut it off. |
| Surface | opaque: the `--bw-overlay` colour at full alpha (never see-through, V4), 1 px `--bw-hairline` contour |

The window's area outside the surface is transparent (the WebView's default background is
transparent and the window extends the DWM frame into its client area, so DWM honours per-pixel
alpha). A click on that transparent shadow room counts as an outside click and closes the flyout.

## 4. Behaviour

| Trigger | Result |
|---|---|
| Click a rail icon (History, Bookmarks, Customize, the rail's "more" button) | opens that flyout beside the button |
| Click the same icon again | closes it (toggle) |
| Click a different rail flyout icon | switches to that flyout |
| Hover a different rail flyout icon while a flyout is open | switches after a 120 ms intent delay (the macOS menu bar rule); leaving the icon earlier cancels |
| Ctrl+H | toggles History, anchored to the History icon |
| Esc | closes it, focus goes back to where it was before (the page, or the rail button that opened it) |
| Click anywhere outside (page, toolbar, tab strip, another app) | closes it; the click goes through to what was clicked |
| Switch tab or workspace, move or resize the window, change DPI | closes it |
| Open another popup (menu, palette, site info) | closes it (the popup takes activation) |
| Open a page from History or Bookmarks in the current tab | navigates, closes, and gives focus to the page; Ctrl+click / middle opens a new tab and keeps the flyout open |

The toggle needs care because the press that toggles is also the press that deactivates the
flyout. When the flyout loses activation to a press on the rail, it stays open and the rail click
that follows decides (same button: close; another flyout button: switch; any other button: close).
The decision therefore never depends on how long the click takes to reach Rust; if no rail command
arrives within 1.5 s (the press became a drag) the flyout closes. A deactivation anywhere else
closes at once and records which flyout closed; a click on that same flyout within 400 ms does not
reopen it (the popup host's reopen guard).

Hover and click combine like a menu bar: when a hover has just switched to a flyout (under
700 ms ago), the click the user was already making on that button confirms it instead of closing
it.

The flyout never steals page scroll: it is its own window, so the wheel over the page still
scrolls the page; the wheel over the flyout scrolls its list (`overscroll-behavior: contain`).

### Keyboard and focus

- Opening activates the flyout window and puts focus in its search field (History, Bookmarks) or
  on the surface itself (Customize: Tab then starts at its first control, and no switch shows a
  focus ring the user did not ask for). Text input works because the popup is activating.
- Arrow keys move through rows (existing roving focus); Down from the search field enters the list;
  Up from the first row returns to search. Enter opens, Ctrl+Enter opens in a new tab, Delete
  removes a history row.
- Tab moves through the controls in order and leaves the list normally; there is no focus trap.
- Esc first clears a non-empty search (existing behaviour), then closes the flyout.
- Browser shortcuts still work from inside the flyout (the panel controller's accelerator hook).

### Motion

- Enter: the surface fades in over `--bw-dur-fast` (83 ms, linear) and slides from the rail side,
  `translateX(-8px)` to 0 over `--bw-dur-base` (167 ms, `--bw-ease-out`). Only opacity and
  transform animate. The window is shown once the page has drawn its first frame, so there is no
  empty or stale frame.
- Hover switch: the window moves at once and the new content runs the same enter.
- Close: the window hides at once (0 ms, within the 83 ms budget). A fading window would still sit
  over the page for 83 ms and could swallow the user's next click, which breaks "never delays input".
- `prefers-reduced-motion: reduce`: no slide, the 83 ms fade only (the global base-layer rule).

### States

- Loading: until the first snapshot the list shows nothing for one frame; the window is shown only
  after the page reported that it painted the snapshot, so there is no visible loading state.
- Empty: History "Pages you visit appear here.", Bookmarks and search empty states (existing, with
  icon); the footer hides when there is nothing to clear.
- Error: a store that cannot be saved still reports through the existing error message box; while a
  modal Bowser message box is up the flyout ignores its own deactivation so it is still there after.

### Appearances and DPI

Light and dark come from the same `state` theme message as every view. The surface uses only
`--bw-*` tokens. DPI: geometry is computed with the button's monitor DPI; a window moved to another
monitor (or a DPI change) closes the flyout, and the next open uses the new monitor's DPI.

## 5. Protocol additions

| Direction | Message | Meaning |
|---|---|---|
| Rust → panel | `{v:1,t:"flyout",caret}` | `caret`: integer, the caret centre in CSS px from the surface top, or `-1` for none. Sent before `panelshow` on every open and switch; restarts the enter motion |
| panel → Rust | `{v:1,t:"panel",a:"shown"}` | the page drew the flyout; Rust shows and activates the window (no new family; `panel` already exists) |
| panel → Rust | `{v:1,t:"panel",a:"dismiss"}` | existing: Esc, close button, or a click in the transparent shadow room |
| rail → Rust | `{v:1,t:"rail",a:"click",k,r?}` | `r` (optional, the hover-layer rectangle grammar) is the button's rectangle in rail CSS px; it anchors the flyout |
| rail → Rust | `{v:1,t:"rail",a:"hover",k,r}` | after the 120 ms intent delay on `customize`, `bookmarks`, `history`, `menu`. The rail cannot tell which flyout is open (Bookmarks is also marked active when the page is bookmarked), so Rust acts only while a different flyout is open |

Rust validates `k` against the rail model and only acts on `hover` while a flyout is open.

## 6. What changes in code

- New `src/win32/flyout.rs`: the window (create, place, show, hide, activation-based dismissal,
  shadow-room geometry) and the pure geometry function with tests.
- `src/win32/html_panel.rs`: the runtime is created with the chrome controllers, its controller
  lives in the flyout window; `toggle`, `open`, `close`, `active_id`. The docked `HtmlPanel`
  `PanelContent` impl and `content_bounds` are deleted (HTML mode no longer docks these panels).
- `src/win32/window.rs`: `rail_activate` routes History, Bookmarks and Customize to the flyout
  when the HTML panel runtime is available, else to the docked native panel; dismissal on tab or
  workspace switch, move and size; the rail's active state follows the flyout.
- `ui/src/panel.ts`: the flyout frame (surface, caret, shadow room, enter motion, focus, `shown`).
- `ui/src/styles/flyout.css`: new surface file.
- `ui/src/rail.ts`: rectangle on click, hover intent while a flyout is open.
- `ui/src/panels/customize.ts`: the "Customize" subtitle becomes the shared panel header (title +
  close), so all three flyouts have one header of the same shape.

Kept: `side_panel.rs` and the native `panel_{history,bookmarks,customize}.rs` for native mode and
as the fallback when the panel controller fails; web app panels stay docked.
