# Downloads (toolbar button, bubble, `bowser://downloads`)

Status: design contract. Goal: downloading in Bowser behaves like Chrome (and Helium, which keeps
Chrome's download bubble): a click saves the file at once, a toolbar button shows progress, a bubble
lists the downloads, `Ctrl+J` and `bowser://downloads` show the full history. Today every download
is cancelled (`DownloadStarting` -> `SetCancel(true)` in `engine/webview2.rs`, `engine/web_app.rs`,
`chrome_host/host.rs`). Private tabs keep cancelling (docs/design/private-tabs.md); so do the chrome's
own webviews.

This file is authoritative for the wire shapes. If an implementer must change one, edit this file
and say so in the report.

## Behaviour (what "like Chrome" means here)

1. **Start.** A download from a normal tab or web app panel saves with no prompt to the download
   folder (default: the user's Downloads known folder). A name clash becomes `name (1).ext`,
   `name (2).ext` (Chrome's rule). With "Ask where to save each file" on, a native Save dialog opens
   first (suggested name and folder preselected); cancelling it cancels the download and records nothing.
2. **Toolbar button.** Right of the address pill, left of the menu button. Hidden when there is
   nothing to show. It appears when a download starts and stays while a download is active or an
   unseen finished one is waiting (a download finished and the bubble has not been opened since).
   Opening the bubble marks them seen; once the bubble closes with nothing active the button hides
   (Chrome hides it again; the list stays reachable from the menu, `Ctrl+J` and the page).
   - Icon: the Download arrow. Active: a circular progress ring around it (aggregate of all active
     downloads, indeterminate sweep when no size is known; paused = ring greyed/static). Finished:
     the ring completes, the arrow morphs to a check for ~1.2 s, then rests as a quiet check state
     (accent dot) while unseen. Failed: warning tone dot.
   - **Start animation**: when a download starts, a small file/arrow glyph drops into the button
     (Chrome's "download started" animation) while the button fades/scales in; the bubble opens.
3. **Bubble** (Chrome's download bubble). Opens automatically under the button when a download
   starts; also toggled by clicking the button (and `Ctrl+J` does NOT open it, it opens the page).
   Auto-opened bubbles close by themselves after ~6 s unless the pointer is over it or focus is in it;
   a bubble the user opened stays until Esc, an outside click, or the button again. Lists up to
   5 most recent downloads (active first, then newest), a header "Downloads", and a footer link
   "Full download history" (opens `bowser://downloads`, shows `Ctrl+J`). Each row: file-type glyph,
   file name (click opens the file when done), a status line, a progress bar while active, and
   buttons by state (see Row states). Live: progress, speed and time left update in place without
   re-rendering the list.
4. **Row states and their actions** (status text in parentheses):
   - active: `12.3 MB of 45.6 MB, 14 sec left` / `12.3 MB` when the size is unknown (Pause, Cancel)
   - paused: `Paused, 12.3 MB of 45.6 MB` (Resume, Cancel)
   - done: `45.6 MB` + `Done` (name opens the file; Show in folder, Remove from list, Copy link)
   - failed/interrupted: `Failed, <reason>` e.g. `Network error`, `Disk full`, `File too large`, `Blocked`
     (Resume when the engine can resume it, else Retry = open the source URL in a new tab; Remove)
   - cancelled: `Cancelled` (Retry, Remove)
   - done but the file is gone: `File moved or deleted` (name struck through, disabled; Remove)
5. **Page.** `bowser://downloads` (`Ctrl+J`, menu row "Downloads", the bubble footer, the
   omnibox). Same family as the history page: sticky header with title, search ("Search downloads"),
   "Open downloads folder", "Clear all" (confirm in the page); day cards (Today, Yesterday, date);
   rows are the bubble rows with more room: bigger glyph, name, the source host/url, status, and
   Show in folder / Open / Remove / Pause / Resume / Cancel / Retry / Copy link. Empty state, no matches
   state, and "downloads could not be read" error state. Live-updates active rows in place.
6. **Settings.** A new "Downloads" section on the settings page: Location (path + "Change"
   button -> native folder picker), and a switch "Ask where to save each file before downloading".
7. **Persistence.** `downloads.txt` in the profile folder (like `history.txt`): finished, failed and
   cancelled entries, newest 500, written on state change (debounced, atomic). Entries active when
   Bowser exits become failed `Interrupted`. A guest window keeps its list in memory only. Private
   tabs never download, so nothing private is ever recorded.
8. **Security.** Pages send only entry ids; Rust looks the id up and acts on the recorded path. A path,
   URL or command from JS is never opened/executed. Open/Show use the shell on the recorded path only
   if it still exists and is inside a regular file. Copy link copies the recorded source URL (http/https
   only). Bubble and page render all file names/URLs with `textContent` (CSP/Trusted Types).
9. **Cleanup on close.** A window closing with active downloads cancels/pauses them (WebView2
   cannot outlive the controller); on next start they show `Interrupted`.

## Wire (JSON, `v:1`, same rules as docs/html-chrome-protocol.md)

### Entry object (Rust -> JS; every key required, no others)

```
{ id: u32 >= 1,
  name: string <= 200 chars (file name only, no path),
  host: string <= 120 (source host, e.g. "example.com"),
  url: string <= 200 (source http(s) URL, truncated by Rust),
  state: "active" | "paused" | "done" | "failed" | "cancelled" | "missing",
  got: integer bytes received so far (>= 0),
  total: integer bytes, -1 when unknown,
  eta: integer seconds left, -1 when unknown,
  bps: integer bytes per second now (0 when not active),
  err: string <= 60 ("" unless failed; short reason text),
  resume: bool (engine can resume it),
  clock: "HH:MM" (local time it finished, or started while active) }
```

`missing` is a finished download whose file no longer exists. The page derives the file-type glyph
from the extension of `name` (image, video, audio, archive, document, code, program, other).

### Toolbar button: Rust -> toolbar host

`{v:1,t:"dlbtn",on:bool,state:"idle"|"active"|"paused"|"done"|"failed",n:int,pct:int,burst:int}`

- `on`: show the button. `n`: active download count. `pct`: aggregate 0-100, -1 unknown.
  `state`: what the icon shows (`done`/`failed` = unseen finished/failed). `burst`: counter that
  increments for every newly started download; the page plays the start animation when it changes
  (never on the first message).
- The toolbar page places the button itself between the pill and the menu button; Rust shrinks
  `pillw` by the button's width (28 px + 4 px gap) while `on` so the layout stays consistent. (Rust
  and the page agree on 32 CSS px: `DL_BUTTON_SLOT`.)

### Toolbar button: toolbar host -> Rust

`{v:1,t:"dl",a:"toggle",text:"x,y,w,h"}` the button was clicked; `text` is its rect in toolbar CSS px
(hover-layer rect grammar). Counts toward the popup command rate limit.

### Bubble (popup host `?view=popup`, page `downloads`)

- Rust -> page: `{v:1,t:"downloads",tok,auto:bool,items:[entry...<=5],more:int,active:int}`.
  Re-posted under the same `tok` on every change (throttled to 4/s while bytes move, immediately
  on a state change). `auto` = opened by a download start (arms the 6 s auto-close). `more` =
  entries beyond the 5 shown.
- page -> Rust: the existing `size` (`tok`,`w`,`h`) and `dismiss` (`tok`), plus
  `{v:1,t:"dlact",tok,id,n}`: `id` is an entry id from the last items (or `0` for list-level
  actions) and `n` is the action: `0` pause, `1` resume, `2` cancel, `3` open file, `4` show in folder,
  `5` remove from list, `6` copy link, `7` retry, `8` open the downloads page (id 0), `9` open the
  downloads folder (id 0), `10` clear all (id 0, page only). Rust checks `tok`, that `id` is a live entry
  (or 0 for 8-10) and that the action is legal for the entry's state, then acts from the deferred queue.
  Actions that leave the list in place do not close the bubble; open/show/page close it.

### Page (`?view=downloads`, `View::DownloadsPage`)

Exact start URL `bowser-ui://app/index.html?view=downloads`; the page never navigates. Accepts
`ready` and the `dpage` family only.

- Rust -> page: `state` (opaque window colour, `solid:true`) and
  `{v:1,t:"snap",p:"downloadspage",d:{state,query,rows,more,undo,ack,active}}`:
  `state`: `on error guest`; `rows`: day headers `{label,date,count}` each followed by `count`
  entries (entry objects above); `more`: bool, older rows were omitted to stay under 240 KiB (the
  history page's rule; the bubble's `more` is a count); `undo`: last
  removal can be undone; `ack`: `none removed restored cleared failed`; `active`: count of active rows.
  Live progress arrives as a fresh snapshot (throttled to 4/s while bytes move, only when it differs).
- page -> Rust: `{v:1,t:"dpage",a,...}`: `search` (`text` <= 256), and by id: `open`, `show`,
  `remove`, `pause`, `resume`, `cancel`, `retry`, `copy` (`id`); bare: `undo`, `clear`, `folder`,
  `refresh`, `settings`.
- Commands are queued and run from `WM_DOWNLOADS_PAGE_COMMAND`, only while a downloads tab is shown.

### Settings snapshot additions

Two keys on the settings snapshot (all keys remain required): `downloadDir` (string <= 260,
display path) and `downloadAsk` (bool). Commands: `{v:1,t:"set",a:"downloadask",on}` and the bare
`{v:1,t:"set",a:"downloadfolder"}` (Rust shows the native folder picker, then answers with the next
snapshot's `ack`). Section id `downloads` is added to `settingsgo`, between `search` and `media` (the
order of `internal_page::Section` and of the page).

## Motion (Opus owns; every animation finite, transform/opacity only)

| What | When | Duration, easing |
|---|---|---|
| Button fades in and scales .85 -> 1 | first download | 167ms out |
| Arrow glyph drops into the button with a soft landing | download start (`burst`) | ~480ms, out + one small settle |
| Progress ring follows `pct` | active | 250ms linear; the implementer picks the technique that respects the transform/opacity-only rule (e.g. two rotating half-ring masks) or documents a narrow exception for `stroke-dashoffset` on this one 20px element |
| Indeterminate sweep | active, size unknown | 1.1s linear loop of a rotating arc, only while active |
| Arrow -> check morph, ring completes | finish | ~300ms then hold 1.2s |
| Bubble enter | open | `.bw-enter.is-down` 167ms |
| New row slides/fades in at top, rows below glide down (FLIP) | new download | 167ms |
| Row progress bar: width via transform scaleX | active | follows data, 250ms linear |
| Finished row: bar collapses, status text crossfades to "Done" | done | 167ms |
| Removed row fades, rows below glide up | remove | 83ms + 167ms |
| Reduced motion | always | fade only, ring jumps |

## Tokens and motion (as built: `ui/src/styles/toolbar.css`, `ui/src/styles/downloads.css`)

Colour: `--bw-*` tokens only, plus local values built from them with the history page's recipes
(`--dp-*` page surfaces, `--db-*` bubble surface, `--dl-*` row). Progress (ring, bars, the check, the
unseen dot) is `--bw-progress`, the page-load colour, so a download reads as "loading" in every theme;
a paused ring or bar is `--bw-text` 40-42%; a failure is the toolbar's warning tone (`--tb-warn`) on the
button and a danger red on rows (`--dl-warn`, proposed `--bw-danger`). File-kind tiles take a tone from
the group palette (image 7, video 0, audio 6, archive 1, document 5, code 3, program 4, other neutral);
a failed, cancelled or missing file loses its colour. Bubble 360 px wide, rows 56 px (page 64 px), the
bar sits in the row's bottom padding so showing or collapsing it never moves anything.

Refinements of the behaviour above (UI only, no wire change):
- Resting unseen state: after the finish morph the button shows the arrow again with a 7 px dot
  (progress tone; warning tone when failed), not a check: the button keeps meaning "downloads".
- The progress ring uses the narrow exception the table allows: `stroke-dashoffset` on the one 28 px
  ring SVG (`pathLength="100"`, offset = 100 - pct; a 0% arc is hidden so its round cap draws no dot).
  Everything else is transform and opacity.
- Auto-close: 6 s after an `auto` opening; the pointer over the bubble holds it; leaving re-arms 3 s;
  a key, press or focus inside makes it a user-opened bubble (no timer). It fades out (83 ms) before
  `dismiss`. An auto-opened bubble never takes the keyboard; one opened by a click focuses itself.
- File names elide in the middle (`Quarterly report — final (appr… v3.pdf`): the extension and the
  stem's last 4 characters never truncate (`nameParts`), like Chrome's `ElideFilename`.
- Actions follow `DownloadManager::allowed`: Copy link in any state with a recorded address, Retry
  only with one, Remove only once a download is no longer running.

| What | When | Duration, easing | Reduced motion |
|---|---|---|---|
| Button fades in, scales .85 -> 1 | `on` becomes true | 167ms out | 83ms fade |
| Button fades out, then `hidden` | `on` becomes false | 83ms linear | same |
| Arrow drops in (falls accelerating from -13px, lands 1.5px low squashed 1.07 x .88, settles at 78%), hands over to the button's own arrow on the last frame | `burst` changes (never the first message) | 480ms | none |
| Landing pulse: a progress-tone ring widens .7 -> 1.2 and fades | 250ms into the drop | 360ms out | none |
| Ring appears, arrow shrinks to .72 inside it | active or paused | 83ms fade + 167ms out / move | fade |
| Arc follows `pct` (dashoffset) | active | 250ms linear (Rust's 4/s, so it glides) | jumps |
| Quarter arc sweeps | active, `pct` -1 | 1.1s linear loop, only while `data-ring="sweep"` | 2.2s loop (it carries status) |
| Finish: arc closes; arrow sinks and fades; check springs in (`--bw-ease-spring`); hold; check shrinks out; ring fades and widens; arrow returns; dot pops in | `state` done after active/paused | 0-250 / 250-550 / hold to 1700 / 1700-2000ms | crossfades only |
| Bubble enters | new `tok` | `.bw-enter.is-down` 167ms | 83ms fade |
| New row drops 6px and fades in; rows below glide (FLIP) | a new id under the same `tok` | 167ms out / move | none |
| Removed row fades where it was (out of flow), rows below glide up | an id leaves | 83ms + 167ms move | 83ms fade |
| Row bar: `scaleX` follows the fraction | active | 250ms linear | jumps |
| Row bar, size unknown: a third of the track slides across | active, total -1 | 1.1s linear loop | 2.2s loop |
| Finished row: bar collapses (`scaleY` 0 + fade), status crossfades | state changes | 167ms in / 167ms fade | fade |
| Secondary row buttons fade in over a feathered backdrop | hover or focus | 83ms | same |
| Page column rises in; the mark's arrow drops into its tray | first snapshot | 167ms out; 560ms | 83ms fade; none |
| Page: new row drops in and glows once | an id arrives under the same search | 167ms + 1.4s | none |
| Page toast (Removed / Cleared, with Undo) | `ack` | 167ms in, 6s hold | fade |

## Files

- Rust core: `src/downloads.rs` (model, naming, persistence, formatting-free), `engine/*` wiring,
  `settings.rs`/`profile.rs`.
- Rust bridge: `src/chrome_host/{protocol,host,downloads_page}.rs`, `src/win32/{downloads_page,html_popup,window}.rs`,
  `src/internal_page.rs`, `src/shortcuts.rs`, menu, `app.rs`.
- UI: `ui/src/downloads-logic.ts`, `downloads-page.ts`, `downloads-bubble.ts`, the shared row
  `downloads-view.ts`, `styles/downloads.css`, toolbar button in `view.ts`/`styles/toolbar.css`,
  settings section, `ui/test/downloads*.mjs` (against `ui/test/fixtures/downloads-messages.json`, which
  Rust writes, and the UI's own `downloads-scenarios.json`). The page is part of `app.js` (not one of
  the lazily imported page bundles).

## Core API (implemented: `src/downloads.rs`, `src/download_shell.rs`, `src/engine/download.rs`, `App`)

What the bridge builds on. Everything runs on the UI thread. Details and unit tests live in the modules.

**Ownership.** `App` holds one `DownloadManager` (`downloads`). A guest window's list is memory only;
a profile whose `downloads.txt` cannot be read is `ListState::Error` (the page's `error` state) and the
file is never rewritten. Engine events reach the manager through `App::on_download_event`, wired in
`start_engine_creation` (tabs) and `panel_webapp::make_events` (web app panels); chrome views in
`chrome_host/host.rs` still cancel, private tabs still cancel (`download_cancelled` is logged).

**Change signal.** `App::set_downloads_observer(app, Some(Rc<dyn Fn(Change)>))`. It runs after every
change with the App unborrowed: `Change::Progress` while bytes move (the engine already limits those to
about 4/s per download), `Change::State` for everything else (start, pause, finish, remove, seen,
missing). Post a window message from it and read the snapshots there. `App::downloads_revision(app)` is
a poll-able counter that grows with every change. The observer is cleared on window close.

**Snapshots (all return the wire JSON):**

| Call | Returns |
|---|---|
| `App::downloads_toolbar(app) -> Toolbar` + `Toolbar::json()` | `{v:1,t:"dlbtn",on,state,n,pct,burst}` |
| `App::downloads_bubble_json(app, tok, auto)` | `{v:1,t:"downloads",tok,auto,items<=5,more,active}` (active first, then newest) |
| `App::downloads_page_json(app, query, ack)` | `{v:1,t:"snap",p:"downloadspage",d:{state,query,rows,more,undo,ack,active}}` |

Notes: `Ack` is `None|Removed|Restored|Cleared|Failed`. On the page `more` is a bool (rows were left
out to stay under 240 KiB, like the history page's; `snapshot_for_page` writes it so); the bubble's
`more` is a count. `d.state` is `on|error|guest`. Rows are day headers
`{label,date,count}` (Today, Yesterday, `YYYY-MM-DD`, from the entry's start time in local time)
followed by their entries, newest first. `n`/`active` count active plus paused downloads; the toolbar
`state` is `active` > `paused` > the latest unseen `failed`/`done` > `idle`; `on` is `state != idle`.
Both bubble and page snapshots re-check finished files first (at most every 2 s), so a deleted file
reads `missing`. Search matches name and URL, case-insensitively.

**Seen.** `App::downloads_mark_seen(app)` when the bubble opens and again when it closes (a download that
finishes while the bubble is open is unseen again until the close). After the close with nothing active
`Toolbar.on` is false, which hides the button.

**Actions.** `crate::downloads::Action::from_code(n)` maps `dlact.n` (0..=10); the page's `dpage` actions
map to the same enum. `App::download_act(app, id, action) -> DownloadFollowup`: the list checks the id
and that the action is legal for the entry's state, then Rust does pause/resume/cancel (engine), open
file, show in folder and open folder itself, on the recorded path only (only if still a regular file; a
failure marks the row `missing`). The bridge handles the rest: `OpenPage` (open `bowser://downloads`),
`OpenTab(url)` (Retry: recorded http(s) URL in a new tab), `CopyText(text)` (Copy link: put it on the
clipboard), `Rejected` (unknown id or illegal action: ignore), `Failed`, `None`. `id` is 0 for
`OpenPage`/`OpenFolder`/`ClearAll`. Clear all keeps running downloads. Undo: `App::downloads_can_undo`,
`App::downloads_undo` (restores the last removal or clear, with its ids; a restored failure cannot resume).
`DownloadManager::act` is the same decision without side effects, for tests.

**Settings.** `Settings.download_dir: String` (empty = Downloads folder; `settings.txt` key `download_dir`,
validated by `settings::parse_download_dir`) and `Settings.ask_download_location: bool` (key
`ask_download_location`). They are stored like every setting, but `chrome_host/settings.rs` is untouched:
its `save` payload does not carry them yet (a save keeps the current values). Helpers for the bridge:
`App::download_dir_display(app)` (the `downloadDir` string: the setting, else the Downloads folder),
`App::set_ask_download_location(app, on)` (`set` `downloadask`), `App::pick_download_folder(app, owner_hwnd)`
(`set` `downloadfolder`: native folder picker, saves the choice, true if it changed). Both setters call
`save_settings`; send the next snapshot afterwards.

**Destination rules** (`downloads::plan_destination`, used by `App::download_plan`): the folder is the
setting if it is an absolute path that exists or can be created, else the Downloads known folder
(`SHGetKnownFolderPath`, fallback `%USERPROFILE%\Downloads`); the name is sanitised (one path component,
no invalid characters, no direction overrides, no reserved device names, no trailing dot or space,
at most 150 UTF-16 units and short enough for the folder) and made unique as `name (1).ext` (`.tar.gz` counts
as one extension; a name already ending in `(1)` becomes `name (1) (1).ext`, like Chrome), also against downloads
still running. With "ask where" on, `IFileSaveDialog` opens first (parented to the window, one at a time;
a second ask while one is open saves without asking); cancelling it cancels the download and records nothing.

**Cleanup.** Closing a tab or web app panel cancels its downloads and lists them `Failed, Interrupted`; a
tab with a download running is never put to sleep by `check_sleep_blocked`; `WM_CLOSE` calls
`App::downloads_shutdown` (marks what is left `Interrupted` and saves). Entries active in the file at
startup load as `Failed, Interrupted`. `downloads.txt` is written on state changes (start, pause, finish,
remove), never on progress.

**Engine surface** (`crate::engine`): `EngineEvents::{download_event, download_plan}` (also on
`WebAppEvents`), `engine::pause_download(key)`, `resume_download(key)`, `cancel_download(key)` (keys come from
`DownloadEvent::Started`; `App::download_act` maps entry ids to them), `engine::download::interrupt_owner`.

## Bridge notes (implemented: `src/win32/{downloads_ui,downloads_page}.rs`, `src/chrome_host/downloads_page.rs`)

The wire above is as built, with these decisions the contract left open (the protocol detail is in
docs/html-chrome-protocol.md, "Downloads"):

- **Page `more` is a bool** (the UI parser and the history page use one); the core writes it so.
- **The button stays while its bubble is open**, even when everything is seen and nothing runs; it hides
  when the bubble closes (which marks finished downloads seen). Before the page has reported the button's
  rectangle (first auto-open) the bubble hangs from the reserved slot left of the menu button.
- **Auto-open is passive.** The popup window is shown with `SW_SHOWNOACTIVATE`, never focused, so typing in
  the page is not interrupted; it closes through the page's own 6 s timer, or the usual popup paths once the
  user clicks into it. It is skipped while another popup is open, while Bowser is not the foreground
  application, while the window is minimised and while the downloads page is the shown tab.
- **Bubble actions that leave the bubble** (open file, show in folder, retry, open the page, open the
  folder) close it first and then act, so the keyboard returns to the page or tab they open. Pause, Resume,
  Cancel, Remove and Copy link keep it open. A refused or failed action keeps it open too.
- **Without the HTML chrome** there is no button, bubble or page: downloads still save, and `Ctrl+J` opens
  the download folder in Explorer.
- **Throttle.** Byte progress reaches the button, bubble and page at most four times a second (a trailing
  timer delivers the last tick); state changes go at once.
- **Settings.** `set` `downloadask` and `downloadfolder` answer with the next snapshot (`ack: saved` after a
  change; a cancelled folder picker changes and says nothing). `save` never carries the download settings.
- **Favicon.** `icons/downloads-favicon.png` is generated (not hand-drawn): the `download` glyph
  (icons/extra/download.svg) at 32 px in the settings gear's slate (#738093) with the history clock's stroke
  weight (about 3 px), antialiased by supersampling.
- **Fixtures.** `ui/test/fixtures/downloads-messages.json` is written by the Rust test
  `downloads::tests::downloads_messages_match_committed_fixture` (`BOWSER_WRITE_FIXTURES=1`): `dlbtn*`,
  `bubble*` and `page*` messages from a list holding every state. The UI's hand-made scenarios live in
  `ui/test/fixtures/downloads-scenarios.json`.

**Not covered headless** (needs a desktop or a live WebView2): the Save and folder dialogs, Explorer
(`ShellExecuteW`, `SHOpenFolderAndSelectItems`), and the real `DownloadStarting` / operation events.
