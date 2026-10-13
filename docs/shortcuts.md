# Keyboard shortcuts

The chord table is `src/shortcuts.rs` (`command_for`, modifiers match exactly). It is consulted from all
three focus contexts:

- **Content WebView**: `AcceleratorKeyPressed` in `src/engine/webview2.rs` posts `WM_SHORTCUT_KEY`.
- **HTML chrome WebViews** (toolbar, tabs, rail, panel): `html_accelerator` in `src/win32/window.rs`
  (via `chrome_host::accel_command`) posts `WM_SHORTCUT_KEY`.
- **Main window / native address EDIT**: `edit_subclass` and the main window call `dispatch_current`.

All three end in `dispatch` in `src/win32/window.rs`. While the HTML omnibox is being edited, only true
editing chords stay with the field: Ctrl+A/C/X/Z, Ctrl+V (not Ctrl+Shift+V), Ctrl+Y, Ctrl+Shift+Z, and
Ctrl/Shift with Left/Right/Home/End/Backspace/Delete (except Ctrl+Shift+Delete).

Ctrl+Shift+V also reached the hidden native address EDIT: TranslateMessage queued a Ctrl+V character
that the EDIT treated as a paste even though the keydown was consumed. `edit_subclass` now drops that
character when Shift is held.

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
| Ctrl+Shift+V | Toggle horizontal/vertical tabs | Bound; fixed: no longer also pastes into the omnibox |
| Ctrl+Shift+Z | Undo tab organisation | Bound (omnibox keeps it for redo while editing) |
| Ctrl+Shift+\ | Toggle split view | Bound |
| Ctrl+Shift+Left / Right | Focus split pane | Bound (omnibox keeps them for selection while editing) |
| Ctrl+Shift+W | Close focused split pane | Bound |
| Ctrl+Shift+K | Command palette | Bound |
| Ctrl+Shift+N | Notes & Tasks | Bound |
| Ctrl+D | Add bookmark | Bound (action runs; feedback is a visual concern) |
| Ctrl+Shift+E | Extensions menu | Bound |
| Ctrl+Shift+P | New private tab | Bound (took this chord from the profile menu; see below) |
| Ctrl+Shift+M | Profile menu | Bound (moved here from Ctrl+Shift+P) |
| Ctrl+Shift+B | Toggle blocker for site | Bound |
| Ctrl+Shift+Y | Picture in picture | Bound |
| Ctrl+Shift+Delete | Privacy dashboard | Bound; fixed: no longer left to the omnibox as a delete chord |
| Ctrl+H | History side panel | Bound (new) |
| Ctrl+J | Downloads | Unbound on purpose: downloads are cancelled by policy, there is no download manager |
| Shift+F10 / Menu key | Page context menu | Not a browser chord; handled natively by the focused page |

## Ctrl+Shift+P and Ctrl+Shift+M

Ctrl+Shift+P was the profile menu. It now opens a **private tab** (the same chord Firefox uses; Chrome and
Edge use Ctrl+Shift+N, which Bowser already binds to Notes & Tasks). The profile menu moved to Ctrl+Shift+M
(Chrome's profile switcher chord). Every place that sent a synthetic Ctrl+Shift+P for the profile menu (the
customise panel's "Switch profile") now sends Ctrl+Shift+M. The palette shows both chords. Ctrl+Shift+P is not
one of the editing chords the HTML omnibox keeps while it is being edited, so it works from the address field.
