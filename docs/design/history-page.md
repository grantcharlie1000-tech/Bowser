# History page (`bowser://history`)

Owner: history. Status: implemented. History is an internal page in a normal tab, like
`chrome://history`, built exactly the way the settings page is (docs/design/settings-page.md). It
replaces the rail's History flyout as the way to see history; the flyout and the docked native
panel stay only as the fallback for a window whose HTML chrome is off or failed.

## How the page is served

- `View::HistoryPage`, start URL exactly `bowser-ui://app/index.html?view=history`, the same
  embedded bundle as every chrome view; `main.ts` routes `?view=history` to `ui/src/history-page.ts`.
- `src/win32/history_page.rs` owns one host per window in a Bowser-owned child window over the
  shown history tab's page area (or its split pane): created on first use, hidden and
  `TrySuspend`ed when no history tab is shown, closed with the window. Same lifetime as
  `settings_page.rs`, which now covers only settings tabs.
- The tab is an ordinary `AppTab` at `bowser://history`, title `History`, favicon the rail clock as a
  32px PNG (`icons/history-favicon.png`, drawn from `icons/rail/clock.svg` in the gear favicon's
  slate and weight; it reads on dark, light and glass strips). The engine never sees `bowser:`.

## URL and entry points (`src/internal_page.rs`)

`bowser://history`: scheme and host case-insensitive, one trailing slash allowed, query and
fragment dropped. Any path, user info or port is not a page; any other `bowser://` host except
`settings` is not a page. The canonical form is what the omnibox shows and the session stores.
`InternalPage::same_page` keeps settings and history tabs apart: opening one never reuses the other.

All entry points go through `history_page::open(owner, app, reuse_blank)`: the rail History button,
Ctrl+H (`Command::ShowHistory` and the window's own key path), the main menu's History row, the
omnibox (`reuse_blank = true`: a blank normal tab becomes the page) and session restore (the stored
address is shown again). An existing history tab in the window is selected, never duplicated. A
history tab is never private: from a blank private tab the page opens in a new normal tab. Without
the HTML chrome, `open` falls back to `window::open_history_panel` (flyout, else docked panel).
There is no command palette entry for History (the palette has none today).

## Data and live refresh

Reading, searching and deleting are the rail panel's: `productivity::read_history_strict` (so an
unreadable file is an error state, never a rewrite), `productivity::search_entries` (the
`search_history` filter: newest first, one case-insensitive substring over address and title,
capped at `HISTORY_LIMIT`, which equals `MAX_HISTORY`, 500), the panel's day-grouping model
(`win32::HistoryModel`) and entry ids (`panels::history::entry_id`), and
`productivity::delete_history_entry`. Undo puts the removed line back with its own timestamp
(`add_history_entry_at`), unless the address was visited again meanwhile.

A visit recorded by `VisitTracker` in this window marks the page dirty (the same `history_dirty`
path that refreshes the flyout); a shown page then gets a fresh snapshot, posted only when it
differs. Showing a history tab again re-reads history, and the page asks for `refresh` when it
regains focus (another window, the Privacy dashboard). Private tabs never write history, guests
never record, and the page reads only the profile's own file, so it shows no private data.

## Bridge and origin policy

Exactly the settings page's: exact start URL for both sources, the per-host token bucket, the
strict JSON parser, and `view_accepts_command(View::HistoryPage, ..)`, which admits only `ready`
and the `hpage` family; no other view accepts `hpage`. Commands are queued and run from
`WM_HISTORY_PAGE_COMMAND`, only while a history tab is shown. `map_command` admits each action
with exactly its own fields (`search` text, `open`/`newtab`/`remove` id; `undo`, `privacy`,
`settings`, `refresh` bare). Wire: docs/html-chrome-protocol.md "History page".

## Layout

One centred column of at most 760px with a 24px gutter (12px below 600px). A sticky header: the
clock mark and "History" with a quiet count (or the search result), Clear browsing data (icon only
below 600px), and a 40px search pill with a `/` hint. Then one card per day: a heading (Today /
Yesterday with the date beside it, older days as "Friday 9 October"), a page count, and rows as
inset pills: favicon (or a letter on a per-site tone), title, host and the rest of the address
(the rest hides below 600px), and the time. On hover or focus the time gives way to two buttons,
Open in new tab and Remove. Off-screen days use `content-visibility: auto`; history is capped at
500 rows, so the whole list is one snapshot (shrunk, with `more`, only if it would pass 240 KiB).

States: no history yet, no matches (with Clear search), history is off (with Open privacy
settings, `settings_page::open(.., Some(Section::Privacy), false)`), guest windows never save
history, and history could not be read (with Try again).

Keyboard: Up/Down, Home/End, PageUp/PageDown move through the rows (one roving tab stop; Left and
Right reach the row's buttons), Enter opens (Ctrl or Shift+Enter in a new tab), Delete removes and
the next row takes the focus, Up from the first row returns to the search, Down or Enter in the
search goes to the rows, `/` or Ctrl+F focuses the search, Escape clears it, Ctrl+Z undoes a
removal while its toast is up. Accessibility: `main` (History), `role="search"`, one `section` per
day labelled by its heading, named icon buttons, a polite live region for the result count and
the toast.

## Tokens and motion

Only `--bw-*` tokens and local `--hp-*` values built with the settings page's recipes (`--hp-page`,
`--hp-card`, `--hp-field`, `--hp-row-hover`, `--hp-hit`, the group palette for letter tiles), so
light, dark, high contrast and the accent follow the browser.

| What | When | Duration, easing | Reduced motion |
|---|---|---|---|
| Column rises 8px and fades in | first snapshot | 167ms out | 83ms fade |
| Clock hands wind back 300deg | page enter | 720ms out | none |
| Header hairline fades in | list scrolled under it | 83ms | fade |
| Row buttons replace the time | hover, focus | 83ms opacity | fade |
| New visit drops in and glows once | a row arrives under the same search | 167ms + 1.4s opacity | none |
| Removed row fades, rows below glide up (FLIP) | remove, undo | 83ms / 167ms move | none |
| "Removed" toast with Undo | Bowser confirms | 167ms in, 6s hold | fade only |

Transform and opacity only, every animation finite, nothing in native paint.
