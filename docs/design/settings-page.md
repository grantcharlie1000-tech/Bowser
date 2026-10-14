# Settings page (`bowser://settings`)

Owner: settings. Status: implemented. Settings is an internal page that opens in a normal tab, like
`chrome://settings` or `edge://settings`. It replaces the Settings dialog (an owned window that
hosted `?view=dialog&p=settings`). The native Settings window stays only as the fallback for a
window whose HTML chrome is off or failed (see "Native fallback").

## How the page is served

The tab never gets a content WebView2. Bowser draws the page itself, in a dedicated chrome host:

- `View::Settings`, start URL exactly `bowser-ui://app/index.html?view=settings`, the same embedded
  bundle as every other chrome view (`ui/dist/index.html`, `app.js`, `app.css`, compiled in by
  `src/chrome_host/assets.rs` and covered by the `ui/dist.manifest` fingerprint). There is no new
  asset, so `build.mjs` keeps one entry point and `main.ts` routes `?view=settings` to
  `ui/src/settings-page.ts`.
- `src/win32/settings_page.rs` owns one host per window, in a Bowser-owned child window placed over
  the tab's page area (the whole viewport, or the tab's pane in a split). It is created on first
  use, hidden and `TrySuspend`ed when no settings tab is shown, and closed with the window or when
  the chrome fails. Pattern and lifetime are the split host's (`split_ui.rs`).
- The tab itself is an ordinary `AppTab` whose address is `bowser://settings[/section]`, title
  `Settings`, favicon the settings gear (an embedded 32px PNG, so every surface that shows a
  favicon - strip, sidebar, hover card, open-tabs menu, palette - shows it with no new wire field).
  `tab_needs_controller` is false for internal addresses and `App::navigate_tab` / `submit_url`
  route them to `App::show_internal_page` before the engine is ever asked, so the content engine
  never sees `bowser:` (the engine itself also only navigates http/https).

Why not a content-profile mapping (`SetVirtualHostNameToFolderMapping`, a `WebResourceRequested`
filter or `NavigateToString` in the tab's WebView2): the privileged bridge would then live in the
same WebView2 profile, process model and navigation history as web pages, its origin could only be
checked by URL string inside a webview that pages can navigate, and it needs changes to
`src/engine/webview2.rs`. The chrome host is already the hardened, page-free environment: InPrivate
`BowserChrome` profile, CSP with Trusted Types, no host objects, no network. Nothing in the engine
changes.

## URL scheme (`src/internal_page.rs`, pure, unit tested)

`bowser://settings` and `bowser://settings/<section>`, section one of `profile`, `appearance`,
`privacy`, `performance`, `search`, `downloads`, `media`, `updates`, `about`. Scheme, host and section are
case-insensitive; one trailing slash is allowed; query and fragment are dropped. User info, a port
or a deeper path are rejected. An unknown section opens the page at the top
(`bowser://settings`). Any other `bowser://` host is not a page: the omnibox reports it as today.
The canonical form is what the omnibox shows, what the session stores and what history would see.

Entry points, all through `settings_page::open(owner, app, section)`:

| From | Behaviour |
|---|---|
| Omnibox (typed or pasted `bowser://settings...`) | an existing settings tab in this window is selected; otherwise a blank normal tab becomes the page; otherwise a new tab opens |
| Main menu "Settings", command palette "Settings" | existing settings tab selected, else a new tab |
| Session restore, a settings tab selected again | the stored address is shown again (no engine) |

A settings tab is never private: from a blank private tab the page opens in a new normal tab.
Leaving the page (typing an address in its tab) clears its title and favicon and navigates
normally. Back and forward do not return to it (it has no engine history).

Sections deep-link both ways. Bowser tells the page which section to show with
`{"v":1,"t":"settingsgo","s":"<section or empty>"}`; a nav click or a search jump sends
`{"v":1,"t":"set","a":"section","text":"<section or empty>"}` and Bowser updates the tab's address.
The page never changes its own URL (no fragments, no `pushState`): the bridge checks the exact
start URL, so a fragment navigation would cut it off.

## Bridge and origin policy

1. Navigation: the host's `NavigationStarting` allows only the exact start URL; every frame is
   cancelled; popups, downloads and permission requests are refused; every resource request is
   answered from the embedded table (404 otherwise), so nothing reaches the network.
2. Every message: both the sender's `Source` and the message's own `Source` must equal the start
   URL byte for byte (`is_trusted_message`), then the token-bucket rate limit, the strict JSON
   parser, and `view_accepts_command(View::Settings, ..)`, which admits only `ready` and the `set`
   command family. A Settings command from any other view is dropped, and the settings view cannot
   send toolbar, tab, panel or popup commands.
3. Runtime gate: commands are queued and run from the window procedure (never inside the WebView2
   callback), and only while a settings tab is the shown tab of that window.
4. Values: `save` must carry every profile setting exactly once, with exact keys and exact values
   (`apply_settings_text`, unchanged rules: unknown keys, duplicates, case variants, out-of-range
   numbers and partial payloads are rejected). Every action checks that no other field is present.
5. Web content cannot reach it: tab navigation is http/https only, external schemes are cancelled,
   popups open only http/https tabs, and a tab's address comes from the engine only as http/https.
   Only Bowser's own entry points create an internal tab.

Commands: `save` (text), `reset`, `openfile`, `autoupdate` (on), `history` (on), `downloadask` (on), `downloadfolder`, `section` (text),
`checkupdates`, `restartupdate`, `privacy`, `profiles`, `import`, `refresh`, `setdefault`. (The
chrome sources never name the password store, `chrome_host/guard.rs`, so the page has no Passwords
row; it stays in the main menu.) The snapshot
(`{"t":"snap","p":"settings","d":{..}}`) carries the profile settings, `historyEnabled`, `guest`,
`profile`, `avatar`, `version`, the update state (`update`, `updateVersion`, `lastCheck`,
`checkResult`, `checking`), `efficiencyActive`, `defaultBrowser` (`yes`/`no`/`unknown`, read-only),
and `ack` (`none`, `saved`, `failed`) with `error`.

Profile settings (`save` lists every one of these, once each, exactly, plus the pre-existing
`tab_layout`, `sidebar_width`, `sleep_enabled`, `sleep_after_secs`, `blocker_enabled`,
`blocker_allow`, `automatic_tab_grouping`, `efficiency_mode`, `tracking_prevention`;
`search_engine` stays optional): `theme` (`helium`/`system`), `theme_accent` (empty or `#rrggbb`),
`toolbar_buttons` (comma list of `back`, `forward`, `reload`, `site-info`, `bookmark`, `newtab`,
`split`, `reader`), `centred_address_bar`, `minimal_url`, `show_load_progress`, `frameless`,
`rounded_content_frame`, `show_home_button`, `bookmarks_bar` (`always`/`never`/`newtab`),
`bookmarks_bar_tab_groups`, `side_panel_position` (`left`/`right`), `tab_preview_images`,
`tab_preview_memory`, `font_size` (`small`/`medium`/`large`), `ui_font`, `default_zoom` (25-500),
`show_zoom_indicator`, `new_tab_beside_active`, `ctrl_tab_mru`, `shift_right_click_context`,
`ctrl_shift_c_copy_link`, `ctrl_s_vertical_tabs`, `pin_new_tab_groups`, `split_edge_drop`,
`block_youtube_video_ads`. Live: theme and accent (the chrome host and the split/history/settings
hosts; the native fallback chrome keeps its fixed colours), the address bar trio, rounded content
frame, the Home button, default zoom and its indicator, new-tab placement, Ctrl+Tab's
most-recently-used order, and split-edge drop. After a restart: `toolbar_buttons`, `frameless`,
the bookmarks bar settings (there is no bookmarks bar yet to show, so these are stored only),
the side panel's dock side, the tab preview settings (no capture exists yet, stored only),
`font_size`/`ui_font` (read by the painted chrome, the HTML toolbar and the address field; the
address field is refreshed on save), and `pin_new_tab_groups` (stored only: tab groups are
implicit per-domain groups with no group-level pin, and the bookmarks bar it pins to does not
exist yet).

`ctrl_shift_c_copy_link`, `ctrl_s_vertical_tabs` and `shift_right_click_context` are live. The two
chords are not in `shortcuts::command_for`, which swallows every key it maps; they go through
`shortcuts::command_for_gated` with the live `GatedChords` (`App::gated_chords`), so a chord whose
setting is off, or Ctrl+S outside the vertical layout, still reaches the page. Ctrl+Shift+C copies
the active tab's address; Ctrl+S collapses or expands the vertical sidebar. With
`shift_right_click_context` on, Shift+Right Click over a tab, group, rail button or the split
divider opens the window's system menu instead of Bowser's own menu.

"Make Bowser the default browser" (`setdefault`, `src/default_browser.rs`): registers Bowser under
`HKEY_CURRENT_USER` only (`StartMenuInternet`, `RegisteredApplications`, the `Bowser.HTML` ProgID
and its `URLAssociations`/`FileAssociations`), then calls
`IApplicationAssociationRegistrationUI::LaunchAdvancedAssociationUI` so Windows' own defaults
picker does the actual switch. No admin, no `HKLM`, and registering alone never makes Bowser the
live default. `defaultBrowser` reads the `http` `UserChoice` ProgID to show the current state; it
is never part of `save`. Any failure (registration or the dialog) answers through the existing
`ack`/`error`.

Private and guest: the page edits the active profile exactly as the dialog did. In a guest window
the history switch is disabled (a guest never saves history);
other values go to the guest's temporary folder, which is deleted when the guest window closes.

History, session, suggestions: `VisitTracker` keeps only http/https, so the page is never a visit.
`session::persistable_url` also accepts the canonical settings addresses, so a restored settings
tab comes back as the page. Settings tabs are left out of the omnibox "Switch to tab" rows, and
`site_security` classifies `bowser:` as an internal page.

## Native fallback

When the window runs the native chrome (HTML chrome off, no chrome environment, or the chrome
failed), there is no trusted host to draw the page, so "Settings" opens the native Settings window
(`settings_panel.rs`) as before, and typing `bowser://settings` does the same. The native window no
longer hosts HTML; the HTML dialog path (`View::DialogSettings`, `?view=dialog&p=settings`) is gone.

## Layout and structure

Two columns from 880px: a 248px sticky sidebar (gear mark and "Settings", the search field, the
section list with a sliding indicator) and a content column of at most 720px. Below 880px the
sidebar becomes a sticky top bar: title and search on one line, the sections as a scrollable row of
chips. Below 560px rows stack their control under the label.

Sections (only settings Bowser really has):

| Path | Title | Cards and rows |
|---|---|---|
| `profile` | You and Bowser | profile card (avatar, name, guest note), Manage profiles (the profile menu), Import from another browser |
| `appearance` | Appearance | Layout (four illustrated tiles, sidebar width slider), Theme (Helium colours/System, accent colour, reset), Toolbar (the buttons as a two-column grid with their glyphs, reset, Home button), Address bar (centred, minimal URL, load progress), Tabs (group by site, new-tab placement, Ctrl+Tab MRU, split edge drop, then previews, pinning and Ctrl+S, which do nothing yet), Window (rounded content frame, frameless), Bookmarks bar (always/never/new tab, tab groups), Helium panels (left/right), Fonts (size, custom font), Zoom (default zoom, indicator), Shortcuts (Shift+Right Click, Ctrl+Shift+C) |
| `privacy` | Privacy and security | Block ads and trackers, Block YouTube video ads, Allowed sites (chip list), Tracking prevention, Save browsing history, Privacy dashboard (embedded: site permissions, statistics, clear browsing data) |
| `performance` | Performance | Efficiency mode (with "active now"), Sleep hidden tabs, Sleep after |
| `search` | Search engine | Optimised, Google, DuckDuckGo (radio cards), custom provider note |
| `downloads` | Downloads | Location (the folder in force, Change opens the native folder picker), Ask where to save each file before downloading (switch); neither is part of `save` (docs/design/downloads.md) |
| `media` | Media | the on-device media support check (runs when the section is first shown) |
| `updates` | Updates | Update automatically, status card with Check for updates or Restart to update |
| `about` | About Bowser | version, Open settings file, Reset settings (in-page confirm; history, the update switch and bookmarks are not touched), Default browser (status, "Make Bowser the default browser") |

Hierarchy is three steps: the section title (Subtitle 20/28), a group heading above its card (Body
Strong 14/20), and the rows inside the card. Space separates them (56px between sections, 24px
between groups, an inset hairline between rows), so a card reads as one object rather than a
stack of strips. Each row: label, one-line description, control (switch, segmented, select,
slider, button). Under the description, a row may carry one quiet note with its own 12px glyph,
never a badge: "Applies after restart" (`restart` in `ROWS`) or "Has no effect yet. Bowser keeps
your choice for when it does." (`notYet`) for a setting that is saved but not acted on: frameless,
the bookmarks bar pair, the tab preview pair, and, until their Rust wiring lands,
`pin_new_tab_groups`, `ctrl_s_vertical_tabs`, `ctrl_shift_c_copy_link` and
`shift_right_click_context`. Flip the row's flag in `settings-page-logic.ts` when one starts
working. A group whose every row is `notYet` (Bookmarks bar, Shortcuts) says so once under its
heading instead of on each row. In a group, the rows that work come first. Changes apply on change, not on a Save
button; Bowser answers with a snapshot and the page shows a small "Saved" toast. Live: layout and
sidebar width, efficiency mode, sleeping tabs, search engine, history, automatic updates. After a
restart: blocker, allow list, tracking prevention, automatic grouping (as before).

Search: filters rows by label, description, keywords and section title as you type (every word
must match, case and accents folded); matching words are marked, empty cards and sections hide
and their nav entries dim, a live count is announced, Enter jumps to and focuses the first match
(its card glows once), Escape clears. Ctrl+F or `/` moves to the search box.

Accessibility: `nav` (Settings sections), `main` (Settings), `role="search"`, one `section` per
section labelled by its `h2`, cards are `role="group"` labelled by their heading, every control is
labelled by its row label and described by its row description, switches are `role="switch"`,
the layout tiles, engines and segmented controls are radio groups with roving focus and arrow
keys, the toast and the result count are polite live regions, and focus rings are the family's.

## Tokens

Only `--bw-*` tokens (`ui/src/styles/tokens.css`) and local `--sp-*` values derived from them in
`ui/src/styles/settings.css`. The host sends the window's `state` (opaque window colour, `solid`,
`dark`, theme inputs), so light, dark, high contrast and the accent follow the browser.

| Local token | Value | Use |
|---|---|---|
| `--sp-page` | `--bw-window-opaque` | page background |
| `--sp-card` | text 4% (dark) / white 72% (light) over the page | cards |
| `--sp-card-edge` | `--bw-hairline` | card border |
| `--sp-nav-active` | `--bw-accent-soft` | nav indicator wash |
| `--sp-mark` | text-grade `<mark>` wash, accent 28% | search hits |

## Motion

Transform and opacity only, every animation finite, nothing in native paint.

| What | When | Duration, easing | Cost | Reduced motion |
|---|---|---|---|---|
| Page rises 8px and fades in | first show of a tab | 167ms out | compositor | 83ms fade |
| Nav indicator slides to the section | active section changes | 250ms move | one element, transform | jumps |
| Section scroll | nav click, search jump | native smooth scroll | compositor | instant |
| Jump target card glow | after a search jump | 900ms fade of a wash layer | opacity | none |
| Switch knob | toggle | 167ms move | transform | instant |
| Layout tile press | click | 83ms scale 0.98 | transform | none |
| Button press | click | 167ms scale 0.97 out | transform | none |
| "Saved" toast | Bowser confirms a save | 167ms in, 1.6s hold, 167ms out | one element | fade only |
| Gear turns one tooth (60deg) | page enter; menu row hover | 600ms / 320ms out | 16-24px SVG | none |

## The gear (`icons/menu/gear.svg`)

Six squared, softly filleted teeth on a 6.6-unit body, tips at 9.3 units (2.7 units of padding),
and a 2.6-unit hub, all on the 24-unit grid with the family's 1.5 stroke, round caps and joins and
`currentColor`. Six teeth (not eight) keep the gaps open at 14-16px, where eight teeth fill in.
Turning by one tooth pitch (60deg) lands on the same drawing, so the hover turn never snaps.
The native menu (`src/win32/menu.rs`, `Icon::Gear`) draws the same outline as a polyline sampled
from the same geometry.
