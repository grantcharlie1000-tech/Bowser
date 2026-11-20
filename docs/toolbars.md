# Toolbars

Settings > Appearance > **Customise your toolbar** opens the toolbar customiser docked beside the page
(`src/win32/toolbar_panel.rs`, `ui/src/panels/toolbar.ts`), so the bar it edits stays in view and changes as each
switch moves. **Reset to default** on the same row puts both bars back.

The customiser is a side panel content (`PanelId::Toolbar`): the side panel host owns its width, its open and close
animation and the page's narrower rectangle, and the content's one child window holds the HTML page
(`?view=dialog&p=toolbar`, `View::DialogToolbar`) through `html_dialog`'s host, like Feeds and Notes. The child is
made once per window and kept, so reopening shows it at once with a fresh snapshot. It sends `tbar` `set` with the
whole new list of one bar (`toolbar_buttons=...` or `toolbar_tabs=...`, checked by `apply_assignment` like every
other writer), and `panel` `dismiss` (its close button, Esc) closes the panel.

The page: an Address bar | Tab bar switch, then one row per button with a switch, grouped by where it sits. Buttons
that are on lead their group in bar order and can be dragged by their grip, or moved with Alt+Up / Alt+Down;
buttons that are off follow. Commands that are off are listed last with a filter and a count of how many are on. There
is no cap on how many buttons a bar holds beyond the wire's (`MAX_ITEMS`, 99, more than every button and command
together). Rows glide to their new place on every change; none of the
motion runs under reduced motion.

While a docked side panel opens or closes, the page follows its edge on every frame (`side_panel::docked_motion`,
the `WM_TIMER` handler in `window.rs`), and a content's HTML child keeps its full width and rides the moving edge
(`side_panel::child_rect`) instead of being re-laid out each frame.

Every item has its own glyph: the bar's own buttons (`ITEM_ICONS`) and every built-in command (`COMMAND_ICONS`, by
command id; `test/bars.mjs` fails when a command has none). A chain shows the `chain` glyph. A command the chrome
sources may not name (the secrets store, `chrome_host::guard`) gets its icon from Rust instead: `toolbar_prefs::command_glyph`
names it, and it travels with the button as `g` (and as `glyph` in the customiser's command list). The toolbar, the tab
strip, the vertical sidebar's rows, the editor and its picker all draw the same one.

Reader, Extensions and Closed tabs appear only when they have something to show. Downloads on the list is pinned: it
shows all the time and opens the downloads bubble even when it is empty. Off the list it still appears, as Chrome's
does, while a download is running or a finished one has not been looked at.

| Bar | Key | Buttons |
|---|---|---|
| Address bar | `toolbar_buttons` | back, forward, reload (before the field), site-info, split, reader (inside it), bookmark, newtab, extensions, downloads (after it; not in the default list); the menu button is always last |
| Tab bar | `toolbar_tabs` | closed-tabs (at the end of the strip, or a row in the sidebar) |
| Panel bar | `toolbar_panel` | feeds (after Bookmarks and History on the rail). **Not offered in the editor:** the rail was removed, so nothing on this list can show. The setting is still read and kept. |

Every bar also takes command buttons: `cmd:<command id>` (any built-in command) and `chain:<n>` (a T4 command chain),
as many as wanted, run through `win32::window::run_registered` like Quick Commands and shortcuts. Rust runs a button
only if the item is at the sent place of that bar's list.

Defaults are today's look; the old default list (`...,bookmark,newtab,...`, which did nothing) is read as the new
default so nobody gets two new buttons. Extensions still appears only when there is something to show; taking it off
the list removes it and gives its room back. Downloads takes its 28 px slot whenever its button shows (pinned, or a
download needs it; `downloads_ui::button`). Always-there buttons at the end of the address bar
(bookmark, new tab, command buttons, any number) need 28 px each (buttons edge to edge, `right_static_px`; the toolbar page
moves the group by `--tb-extra`). The layout makes that room (`Arrangement::bar_room`, `LayoutOptions::bar_room`): the
omnibox group widens out of the toolbar's spare space first, and in `SingleRow` out of the tabs' room (down to their
minimum), so the address field keeps its width; only when the window has nothing left to give does the field get
narrower. A change of the list lays the window out again (`html_sync`). Back, forward and reload work the other way: the layout keeps room for all
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
