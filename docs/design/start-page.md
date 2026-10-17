# Start page (new tab, speed dials, dashboard widgets, homepage, startup)

Owner: start page. Status: implemented. A blank new tab shows the start page: speed dials (bookmarks you
pin, in groups) and, if you turn it on, a dashboard of widgets. Settings has a Start page section
(`bowser://settings/startpage`) with the homepage and what Bowser opens at startup.

## How it is served

- The split view's one HTML host (`View::Split`, start URL `bowser-ui://app/index.html?view=split`) shows
  the page over a blank normal tab, exactly as it shows the private landing over a blank private tab
  (`src/win32/split_ui.rs`, `Shown::Start`). No extra WebView2 is created for it, and the tab keeps no
  content WebView2 of its own: a tab with no address and no controller is "blank", and a blank tab is the
  start page. Navigating the tab (or opening a page from it) ends the page.
- The page code is its own bundle, `dist/start-page.js` (`ui/src/start-page.ts`, `start-page-logic.ts`,
  `styles/start.css`). `app.js` names it but does not contain it; the host imports it the first time Rust
  sends a start snapshot, so no other view parses it.
- Rust: `src/start_page.rs` (model: settings, speed-dial groups, widgets, pure decisions),
  `src/start_page/service.rs` (snapshot building and every command against the profile's files, tested
  without a window), `src/chrome_host/start_page.rs` (wire: snapshot JSON, `map_command`),
  `src/win32/start_ui.rs` (the window side: when a snapshot is due, opening an address, the image picker
  and re-encode, the Notes panel, the settings page).
- The "new tab page" choice (start page / homepage / blank / an address) belongs to the tabs settings
  (T1). `start_ui::enabled()` is the one place that choice gates the page; until it lands a blank normal
  tab always shows the start page. A private tab keeps its own landing; a split's empty side keeps the
  picker; a window without HTML chrome keeps the plain new-tab surface it always had (the GDI fallback
  has no start page, and `settings_panel.rs` keeps every start setting's value when it saves).

## Speed dials are bookmarks

A speed dial is a link bookmark. A **group** is a bookmark folder flagged in `speeddial.txt`
(`group<TAB>id`, in tab order, plus `last<TAB>id` and `thumb<TAB>id<TAB>id.jpg`); its links are the dials
and its sub-folders open in place with a path (Work / Docs) and an Up button. Add, edit, remove, reorder
and move into a folder are the bookmark store's own add, update, delete and move (`bookmarks-v2.txt`), so
the Bookmarks panel and the start page always agree. "Remove" therefore deletes the bookmark, and the
confirmation says so. A group can be a new folder or an existing bookmark folder ("Or use a bookmark
folder"); removing a group from the page only unflags it, the folder and its sites stay in Bookmarks.

- Tile faces: the chosen image, else the site's cached favicon (`favicon::load_favicon_bytes`, sent as
  PNG data URIs, at most 80 hosts and 100 KiB), else a letter on a tone. No page screenshots exist in
  Bowser, so a tile is never a thumbnail of the page.
- A chosen image: the system open dialog (png, jpg, gif, bmp, tif), decoded with WIC, fitted inside
  480 x 300 without enlarging, laid on white, re-encoded as a JPEG of at most 96 KiB into
  `speeddial-thumbs/<bookmark id>.jpg`. Nothing is uploaded or fetched. The images follow the page in
  `thumbs` messages (each under the page's 256 KiB limit) so the first paint never waits for one.
- Drag to reorder: pointer drag (6 px to start) before/after a tile, onto a folder tile's middle to put it
  inside; Escape cancels. Keyboard: Alt+Left/Right moves the focused dial, Delete asks to remove it, arrow
  keys move focus. The `start_drag_reorder` setting turns pointer and keyboard moves off.
- Limits: 24 groups, 120 dials listed per folder, 200 folders offered in pickers.

## Widgets

`start-widgets.txt` lists, orders and configures them (an absent file shows the date, top sites and privacy
statistics). All widget data is read from the profile: no new network service, no account.

| Widget | Shows |
|---|---|
| Top sites | hosts ranked by how many of their pages are in history (one line per address), newest page linked |
| Bookmarks | a folder you choose, as titles and favicons (list) or tiles (favicon, or the image you chose) |
| Sticky note | a note saved as you type (up to 2000 characters); "Save to Notes" adds it to the Notes panel |
| Date | the local date, drawn by the page |
| Privacy statistics | blocked today, blocked in all, and the host blocked most (`privacy-stats.txt`) |
| Webpage | an http(s) address you type, in a sandboxed frame (see Security) |

Settings: show the dashboard, columns 1 to 4 (the window width can lower it), regular or tall, background
solid / semi-transparent / transparent / custom colour (the text colour is chosen to keep 4.5:1), drag to
rearrange (pointer on the handle, or Alt and an arrow key).

## Settings keys (`settings.txt`)

All are optional in a `save` payload (an older page leaves them as they were) and tolerant in a hand-edited
file (a value that does not parse is ignored; numbers are clamped).

| Key | Values | Acts on |
|---|---|---|
| `start_group_reopen` | `last`, `first` | which group a new tab opens on |
| `start_titles` | `always`, `auto`, `never` | when a dial shows its name (`auto`: under tiles with no chosen image, on hover for the rest) |
| `start_add_button` | bool | the "Add a site" tile |
| `start_columns` | 1 to 12 | most dials in a row (the width can lower it) |
| `start_thumb_size` | 1 to 5 | tile width 96, 120, 148, 184, 232 px |
| `start_drag_reorder` | bool | pointer and keyboard moves |
| `homepage` | http(s) address or empty | the rail's Home button opens it in the current tab; empty keeps "new tab" |
| `startup` | `start_page`, `last_session`, `homepage`, `specific_pages` | the first window |
| `startup_pages` | up to 8 http(s) addresses, space separated | `specific_pages` |
| `start_widgets` | bool | the dashboard |
| `start_widget_columns` | 1 to 4 | widgets per row |
| `start_widget_height` | `regular`, `tall` | 208 or 384 px |
| `start_widget_bg` | `solid`, `translucent`, `transparent`, `custom` | the card fill |
| `start_widget_color` | `#rrggbb` | the `custom` fill |
| `start_widget_drag` | bool | widget moves |

Startup: `start_page` leaves the first tab blank; `homepage` and `specific_pages` navigate it (one tab per
page, the first shown) unless the launch already has a target (a restored crash session, a command-line
address or a benchmark URL); `last_session` restores a clean exit's tabs without the crash prompt
(`SessionStore::open_with`), also unless the launch carries an address. An empty homepage or page list
falls back to the start page.

## Performance: a new tab must not wait on this

- The page is data, not a navigation: the host exists already (created once, on the first start page,
  and not before the process is 1.5 s old, so a cold launch finishes the toolbar and tab strip first; a
  blank tab shows the plain new-tab surface until the page is ready). Idle hosts close after three minutes
  hidden, as before, and the next blank tab creates it again.
- `start_ui::due` reads no file for an unchanged page: it compares the settings and five file stamps (at
  most every 750 ms) and rebuilds only when one changed or a command ran. A rebuild parses the bookmark
  file (its duplicate-id check is now a hash set instead of a scan, which made it quadratic), and reads
  history and statistics only for a widget that shows them.
- Chosen images and the lazily loaded bundle keep the first paint small. Timings are in the PR.

## Security notes

- Every command is a flat `{t:"spage",a,...}` message in the panel family (at most 4 fields beyond
  `v` and `t`), accepted only from the split host (`view_accepts_command`), and checked again in Rust:
  each action only with exactly its own fields, ids resolved against the live data, addresses http(s)
  without user info, lengths capped. Page text reaches the DOM through `textContent` only.
- **Webpage widget.** The one place the chrome embeds a page. The page makes an `<iframe>` only for an
  address the user typed into that widget (re-validated as http(s) in the page and in Rust), with
  `sandbox="allow-scripts allow-forms"` (a unique origin: no cookies or storage, no same-origin access,
  no top navigation, popups, downloads or modals), `referrerpolicy="no-referrer"`, `allow=""` and
  `loading="lazy"`. Two chrome-host rules had to change for it, both for the split host only:
  its document is served with `frame-src http: https:` (`assets::SPLIT_RESPONSE_HEADERS`; scripts,
  styles, images, connections, forms and objects stay as locked down as in every view), and a subframe
  of that host may navigate to an http(s) address (`assets::split_frame_may_navigate`; every other host,
  every other scheme and every top-level navigation is still cancelled). Messages from inside the frame
  fail the bridge's exact-source check, new windows, downloads and permission requests are still
  refused, and the chrome profile is InPrivate. The frame's site sees an ordinary request (its IP and
  user agent) each time the page is shown: that is the cost of the feature, and it only happens for a
  widget the user added.
- The thumbnail picker reads one file the user picked (at most 16 MiB), decodes it with the system
  imaging component, and keeps only the re-encoded JPEG.

## Not ported

- Vivaldi options with no meaning here are skipped, and nothing was added dead: there is no
  "open in a new tab with the speed dial group bar" variant, no page-screenshot thumbnails (Bowser takes
  none), and no search field on the page (the address bar is focused on a new tab, as before).
- The group tab bar reorders groups from its menu (Move left, Move right), not by dragging.

## Tests

- Rust: `start_page::tests` (settings, groups, dial view, thumbnails, widgets, top sites),
  `start_page::service::tests` (each command, file, snapshot; reopen last/first; widgets; guest; a
  new-tab cost check on a large profile), `chrome_host::start_page::tests` (snapshot, fixture
  `ui/test/fixtures/start-page-messages.json`, command grammar), `settings`/`chrome_host::settings`
  (keys round trip, save payload), `app::tests::start_page_flow` (startup plans), `session::tests`
  (continue where you left off), `win32::start_ui::tests` (the image re-encode).
- UI: `ui/test/start-page.mjs` (contract, decisions, page in a fake document: every click, key and drag),
  `ui/test/settings-page.mjs` (the Start page section: each control saves its own key).
- Headless screenshots: `benchmarks/results/sp/` (light and dark, 100% and 150%).
