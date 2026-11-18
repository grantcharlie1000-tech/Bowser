# Toolbars

Settings > Appearance > Toolbars edits one bar at a time (Address bar | Tab bar): a live preview of the bar, the
list of its buttons (move up, move down and remove come up on hover or focus; an arrow that cannot move a button is
left out), one-click chips for the bar's own buttons that are off it, and a picker for any command or chain. Each
change is the whole new list and acts at once. Rules: `src/toolbar_prefs.rs` (Rust authority) and
`ui/src/bars-logic.ts`; the editor is `ui/src/toolbar-editor.ts`.

Every item has its own glyph: the bar's own buttons (`ITEM_ICONS`) and every built-in command (`COMMAND_ICONS`, by
command id; `test/bars.mjs` fails when a command has none). A chain shows the `chain` glyph. The toolbar, the tab
strip, the vertical sidebar's rows, the editor and its picker all draw the same one.

Buttons that appear only when they have something to show (Reader, Downloads, Extensions, Closed tabs; `SHOWS_WHEN`)
say so under their name in the editor and are drawn dashed in its preview, so a button just added never looks lost.

| Bar | Key | Buttons |
|---|---|---|
| Address bar | `toolbar_buttons` | back, forward, reload (before the field), site-info, split, reader (inside it), bookmark, newtab, extensions, downloads (after it); the menu button is always last |
| Tab bar | `toolbar_tabs` | closed-tabs (at the end of the strip, or a row in the sidebar) |
| Panel bar | `toolbar_panel` | feeds (after Bookmarks and History on the rail). **Not offered in the editor:** the rail was removed, so nothing on this list can show. The setting is still read and kept. |

Every bar also takes command buttons: `cmd:<command id>` (any built-in command) and `chain:<n>` (a T4 command chain),
at most 4 per bar, run through `win32::window::run_registered` like Quick Commands and shortcuts. Rust runs a button
only if the item is at the sent place of that bar's list.

Defaults are today's look; the old default list (`...,bookmark,newtab,...`, which did nothing) is read as the new
default so nobody gets two new buttons. Downloads and Extensions still appear only when there is something to show;
taking them off the list removes them and gives their room back. Always-there buttons at the end of the address bar
(bookmark, new tab, command buttons, at most 5) take 32 px each from the address field (`right_static_px`; the toolbar
page moves the group by `--tb-extra`). Back, forward and reload work the other way: the layout keeps room for all
three, and whatever the list leaves empty goes to the address field, which grows left over it and keeps its right
end (`left_free_px`, `chrome_host::toolbar_css_placement_with_room`). With none of them, the field starts at the
toolbar's edge.

## Left out

- **Status bar**: Bowser has no status bar (Settings > Appearance > Status bar only hides WebView2's link overlay), so
  there is nothing to edit.
- **Per-extension buttons** ("only hidden / all", expanding hidden ones): WebView2 exposes no extension action icons or
  badges, so Bowser has one Extensions button whose menu already lists the extensions; it can be added or removed.
- **Built-in rail buttons** (Home, workspaces, web apps, Bookmarks, History) are still shown or hidden in the Customize
  panel; the panel bar editor adds buttons after them.
- The native GDI fallback keeps its fixed buttons; its settings window keeps the editor's extra buttons when it saves.
