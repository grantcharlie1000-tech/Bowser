# Toolbars

Settings > Appearance > Edit toolbars lists the buttons of three bars; each change is the whole new list and
acts at once. Rules: `src/toolbar_prefs.rs` (Rust authority) and `ui/src/bars-logic.ts`.

| Bar | Key | Buttons |
|---|---|---|
| Address bar | `toolbar_buttons` | back, forward, reload (before the field), site-info, split, reader (inside it), bookmark, newtab, extensions, downloads (after it); the menu button is always last |
| Tab bar | `toolbar_tabs` | closed-tabs (at the end of the strip, or a row in the sidebar) |
| Panel bar | `toolbar_panel` | feeds (after Bookmarks and History on the rail) |

Every bar also takes command buttons: `cmd:<command id>` (any built-in command) and `chain:<n>` (a T4 command chain),
at most 4 per bar, run through `win32::window::run_registered` like Quick Commands and shortcuts. Rust runs a button
only if the item is at the sent place of that bar's list.

Defaults are today's look; the old default list (`...,bookmark,newtab,...`, which did nothing) is read as the new
default so nobody gets two new buttons. Downloads and Extensions still appear only when there is something to show;
taking them off the list removes them and gives their room back. Always-there buttons at the end of the address bar
(bookmark, new tab, command buttons, at most 5) take 32 px each from the address field (`right_static_px`; the toolbar
page moves the group by `--tb-extra`).

## Left out

- **Status bar**: Bowser has no status bar (Settings > Appearance > Status bar only hides WebView2's link overlay), so
  there is nothing to edit.
- **Per-extension buttons** ("only hidden / all", expanding hidden ones): WebView2 exposes no extension action icons or
  badges, so Bowser has one Extensions button whose menu already lists the extensions; it can be added or removed.
- **Built-in rail buttons** (Home, workspaces, web apps, Bookmarks, History) are still shown or hidden in the Customize
  panel; the panel bar editor adds buttons after them.
- The native GDI fallback keeps its fixed buttons; its settings window keeps the editor's extra buttons when it saves.
