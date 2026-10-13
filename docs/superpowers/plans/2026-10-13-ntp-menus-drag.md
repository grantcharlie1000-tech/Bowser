# NTP card, tab overflow slot, drag settle, cursor composers

## Rulings

- Work stays on `main`. The user asked for these fixes in this checkout and for screenshots of the running app. No branch, no commit unless they ask.
- Remove the whole centred new-tab card (title, search pill, magnifier, card fill). Keep the empty viewport fill and the page's rounded edge. Do not touch the toolbar omnibox.
- The tab overflow chevron keeps a real slot. The HTML tab strip stops before it, including the island gap, so the last tab is pushed clear.
- New workspace, add web app, and edit web app are compact menus anchored at the pointer, in the same visual language as the rail context menu. They are not centred dialogs.
- A new workspace is created only when the user confirms a name. Dismiss creates nothing.
- Enter submits, Escape dismisses, fields have names. `prefers-reduced-motion` snaps.

## Tasks

1. Remove the new-tab search card in `paint_new_tab_surface`.
2. Shrink `tab_row_bounds` when `tab_menu_button` is present, and update `tab_row_bounds_is_the_full_tab_row`.
3. Fix tab-drag release so a drop does not animate home and then to the destination. Authority: `docs/design/ntp-menus-motion.md`.
4. Cursor composers for new workspace, add web app, and edit web app. Authority: the same design note.
