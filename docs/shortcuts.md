# Keyboard shortcuts

The chord table is `src/shortcuts.rs` (`command_for`, modifiers match exactly). It is consulted from all
three focus contexts:

- **Content WebView**: `AcceleratorKeyPressed` in `src/engine/webview2.rs` posts `WM_SHORTCUT_KEY`.
- **HTML chrome WebViews** (toolbar, tabs, rail, panel): `html_accelerator` in `src/win32/window.rs`
  (via `chrome_host::accel_command`) posts `WM_SHORTCUT_KEY`.
- **Main window / native address EDIT**: `edit_subclass` and the main window call `dispatch_current`.

All three end in `dispatch` in `src/win32/window.rs`. Modifiers match exactly, so Ctrl+Alt chords never run a command except Ctrl+Alt+R (`ctrl_alt_r_is_the_only_ctrl_alt_chord` in `src/shortcuts.rs`). While the HTML omnibox is being edited, only true
editing chords stay with the field: Ctrl+A/C/X/Z, Ctrl+V (not Ctrl+Shift+V), Ctrl+Y, Ctrl+Shift+Z, and
Ctrl/Shift with Left/Right/Home/End/Backspace/Delete (except Ctrl+Shift+Delete).

Ctrl+Shift+V is not a Bowser chord: the tab layout is chosen only in Settings > Appearance
(`bowser://settings/appearance`). Settings has no shortcut of its own (Ctrl+, is Passwords and
passkeys); it opens from the main menu, the command palette or by typing `bowser://settings`. On the
settings page, Ctrl+F or `/` moves to its search box.

| Shortcut | Action | Status |
| --- | --- | --- |
| Ctrl+T | New tab, focus address | Bound |
| Ctrl+W, Ctrl+F4 | Close tab | Bound |
| Ctrl+Tab, Ctrl+PgDn / Ctrl+Shift+Tab, Ctrl+PgUp | Next / previous tab | Bound |
| Ctrl+1..8, Ctrl+9 | Select tab N / last tab | Bound |
| Ctrl+L, Alt+D, F6 | Focus address field | Bound |
| F5, Ctrl+R, Refresh key | Reload | Bound |
| Ctrl+F5, Ctrl+Shift+R | Hard reload | Bound |
| Esc, Stop key | Stop loading | Bound (HTML omnibox keeps Esc to cancel edits) |
| Alt+Left / Alt+Right, Back / Forward keys | History back / forward | Bound |
| Ctrl+, | Passwords & Passkeys | Bound |
| Ctrl+Shift+L | Fill saved password | Bound |
| Ctrl+Shift+Z | Undo tab organisation | Bound (omnibox keeps it for redo while editing) |
| Ctrl+Shift+\ | Toggle split view | Bound |
| Ctrl+Shift+Left / Right | Focus split pane (a pair: left / right, top / bottom when stacked; three or four panes: the neighbouring pane to the left / right) | Bound (omnibox keeps them for selection while editing) |
| Ctrl+Shift+Up / Down | Focus the top / bottom pane of a stacked split, or the pane above / below in a layout of three or four panes | Bound only while a split with panes above one another is shown; otherwise the page keeps them (paragraph selection) |
| Ctrl+Shift+W | Close focused split pane (in a layout of three or four panes its sibling expands) | Bound |
| Ctrl+Shift+K | Command palette | Bound |
| Ctrl+Shift+N | Notes & Tasks | Bound |
| Ctrl+D | Add bookmark | Bound (action runs; feedback is a visual concern) |
| Ctrl+Shift+E | Extensions menu | Bound |
| Ctrl+Shift+P | New private tab | Bound (took this chord from the profile menu; see below) |
| Ctrl+Shift+M | Profile menu | Bound (moved here from Ctrl+Shift+P) |
| Ctrl+Shift+B | Toggle blocker for site | Bound |
| Ctrl+Shift+Y | Picture in picture | Bound |
| Ctrl+Shift+Delete | Privacy dashboard | Bound; fixed: no longer left to the omnibox as a delete chord |
| Ctrl+H | History page (`bowser://history` in a tab; an open history tab is selected, never duplicated). The History side panel when the window has no HTML chrome | Bound |
| Ctrl+Plus, Ctrl+=, Ctrl+Numpad Plus | Zoom in one step (25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500) | Bound (new); stored per origin in `site-prefs.txt`; Ctrl+Shift+= also zooms in |
| Ctrl+Minus, Ctrl+Numpad Minus | Zoom out one step | Bound (new) |
| Ctrl+0, Ctrl+Numpad 0 | Reset zoom to 100% | Bound (new) |
| Ctrl+mouse wheel | Zoom in / out | Not a chord: WebView2's own page zoom stays on, and `ZoomFactorChanged` stores the result like a key press |
| Ctrl+Alt+R | Reader view on / off | Bound. The only Ctrl+Alt chord; on layouts where AltGr reports as Ctrl+Alt, AltGr+R also toggles it. A held chord does not repeat |
| Esc | Leave reader view | Only while reader view is on (the content WebView reports Escape to the app and still passes it to the page); otherwise unchanged |
| Ctrl+J | Downloads page (`bowser://downloads` in a tab; an open downloads tab is selected, never duplicated). The download folder in Explorer when the window has no HTML chrome | Bound. The toolbar button, not this chord, opens the bubble |
| Shift+F10 / Menu key | Page context menu | Not a browser chord; handled natively by the focused page |

## Page zoom

`ZoomIn`, `ZoomOut` and `ZoomReset` run `App::zoom_active` (`src/app.rs`), which steps the active tab
with `ICoreWebView2Controller::SetZoomFactor` through `site_prefs::ZoomStep` (Chrome's levels, 25% to
500%). The new value is stored for the tab's origin in `site-prefs.txt`, so the next navigation to that
origin, or a restart, shows the same zoom. At each navigation commit (`SourceChanged`) the app sets the
tab to the stored zoom of the new origin, or 100% for a site with none. A zoom the page changes itself
(Ctrl+wheel, handled by WebView2) arrives as `ZoomFactorChanged`; it is stored the same way, and the
events for zooms the app set itself are recognised and ignored. Guest sessions keep the values in
memory only. The toolbar shows a chip with the percentage while the page is not at 100%; the chip and
the Site information Reset button both run `ZoomReset`.

Ctrl+Plus on most layouts is Ctrl+Shift+=, so both are bound; Ctrl+Alt chords (AltGr) stay unbound as
before. While the HTML omnibox is being edited these chords still zoom the page, as in Chrome.

## Ctrl+Shift+P and Ctrl+Shift+M

Ctrl+Shift+P was the profile menu. It now opens a **private tab** (the same chord Firefox uses; Chrome and
Edge use Ctrl+Shift+N, which Bowser already binds to Notes & Tasks). The profile menu moved to Ctrl+Shift+M
(Chrome's profile switcher chord). Every place that sent a synthetic Ctrl+Shift+P for the profile menu (the
customise panel's "Switch profile") now sends Ctrl+Shift+M. The palette shows both chords. Ctrl+Shift+P is not
one of the editing chords the HTML omnibox keeps while it is being edited, so it works from the address field.
