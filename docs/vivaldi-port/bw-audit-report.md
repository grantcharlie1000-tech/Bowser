# Bowser settings audit (origin/main @ 8c638853, read-only, code and tests only)

Chain traced: `ui/src/settings-page-logic.ts` (ROWS/SECTIONS, `saveText`) -> `ui/src/settings-page.ts` (controls) -> `{t:"set",a:"save"}` -> `src/chrome_host/settings.rs` (`map_command`, `apply_settings_text`, `snapshot_json`) -> `src/win32/settings_page.rs::run(Action::Save)` -> `crate::settings::save` + `App::apply_tab_layout_settings / apply_efficiency_mode / apply_sleep_settings / apply_live_settings` (app.rs ~6440) -> consumers.

Only `tab_layout`, `sidebar_width`, efficiency, sleep and the keys copied in `apply_live_settings` change the running app. The rest are read at startup (blocker, tracking, tab grouping) or from the file on use (search engine, YouTube ads, side panel position).

Test column. UI = `ui/test/settings-page.mjs`. It covers snapshot parsing, `saveText` output and the row index for every key. It never proves behaviour, so it is not listed per row. "contract only" = UI/Rust round-trip tests exist, no behaviour test.

## Table (one row per ROWS entry; keys are settings.txt keys)

| key (row id) | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| profile | Profile | `chrome_host/settings.rs::snapshot_json` (profile/avatar/guest, read-only display) | UI | WORKS | display only |
| profiles | Manage profiles | `win32/settings_page.rs::run(Action::Profiles)` -> `window.rs::show_profile_menu_from_page` | none found | WORKS | action row |
| import | Import from another browser | `settings_page.rs::run(Action::Import)` -> `import_panel::open` | none in settings chain | WORKS | action row |
| layout (tab_layout) | Browser layout | `app.rs::apply_tab_layout_settings` (live), `win32/window.rs` layout compute, `chrome/layout.rs` | Rust settings.rs round-trip + layout.rs tests | WORKS | 4 layouts: horizontal, tabs-top, single-row, vertical |
| sidebarWidth (sidebar_width) | Sidebar width | `app.rs::sidebar_width` / `Settings::effective_sidebar_width`, window.rs layout | Rust settings.rs clamp tests, layout.rs | WORKS | vertical layout only |
| theme (theme) | Theme | `app.rs::settings_accent` -> `theme::resolved_accent` | Rust theme.rs:668 | WORKS | helium or system accent |
| themeAccent (theme_accent) | Accent colour | `theme::resolved_accent` via `App::settings_accent`; `chrome_host/protocol.rs` theme tokens | Rust theme.rs:668, protocol.rs:4202 | WORKS | |
| toolbarButtons (toolbar_buttons) | Customise your toolbar | `win32/window.rs::PaintPrefs::read` -> `chrome/paint.rs` (`show_site_info`, `show_split` only) | Rust settings.rs parse test, UI | PARTIAL | only `site-info` and `split` are read, and only by the GDI-painted omnibox. `back`, `forward`, `reload`, `bookmark`, `newtab`, `reader` are saved and never read. The HTML toolbar (`ui/src/view.ts`, `ViewState` in protocol.rs) has no field for any of them |
| showHomeButton (show_home_button) | Show the Home button | `win32/window.rs:1287` adds `RailKey::Home` rail entry | none | WORKS | adds a rail button (label says "toolbar", it is the rail); Home = new tab (window.rs:4836) |
| centredAddressBar (centred_address_bar) | Centre the address bar | `chrome/layout.rs::compute_in_frame_for_with` via window.rs:931; HTML gets the pill rect from it | Rust layout.rs:1756 | WORKS | |
| minimalUrl (minimal_url) | Show a simplified address while unfocused | `omnibox.rs::address_display`, `window.rs::sync_address_input` | none found | WORKS | no test names the setting |
| showLoadProgress (show_load_progress) | Show a loading progress bar | `window.rs:8176/9027` -> `ViewState.show_progress` | protocol.rs:3242 (clamp only) | WORKS | |
| grouping (automatic_tab_grouping) | Group tabs by site | `app.rs:2479` -> `tab_organisation::apply_membership` | Rust app.rs:8488 | WORKS | startup-read (not in `apply_live_settings`), matches the "restart" flag |
| newTabBesideActive (new_tab_beside_active) | Open new tabs next to the active tab | `app.rs::new_tab_in_mode` -> `tabs.rs::add(after_active)` | Rust tabs.rs:373/383 | WORKS | |
| ctrlTabMru (ctrl_tab_mru) | Cycle tabs MRU first with Ctrl+Tab | `app.rs::select_next/select_prev/select_mru_other` | Rust app.rs:8235 | WORKS | |
| splitEdgeDrop (split_edge_drop) | Allow dropping a dragged tab on the window edge to split | `win32/split_ui.rs:725` | none found | WORKS | |
| tabPreviewImages (tab_preview_images) | Show a thumbnail in tab previews | none (only settings.rs, chrome_host/settings.rs, GDI panel) | contract only | NOTHING | marked notYet. No tab preview card exists in `ui/src/hover.ts` / `tooltip.ts` (tooltips only) |
| tabPreviewMemory (tab_preview_memory) | Show memory use in tab previews | none | contract only | NOTHING | marked notYet |
| pinNewTabGroups (pin_new_tab_groups) | Pin new tab groups to the bookmarks bar | none (copied in `apply_live_settings` only) | contract only | NOTHING | marked notYet; depends on a bookmarks bar that does not exist |
| ctrlSVerticalTabs (ctrl_s_vertical_tabs) | Ctrl+S collapses or expands the sidebar | `app.rs::gated_chords` -> `shortcuts.rs:235` -> `window.rs::toggle_sidebar` | Rust host.rs:1686, shortcuts.rs:518-530 | MISMATCH | works, but row still carries `notYet` so the page says "Has no effect yet" |
| roundedContentFrame (rounded_content_frame) | Rounded corners around the page | `window.rs:10201` -> `paint.rs:640/651` (`content_rounded`, 9 dip radius) | pixel tests use `content_rounded: true` only | WORKS | the page boundary is natively painted, so it applies in HTML mode too; the off state is not tested |
| frameless (frameless) | Frameless window | none. No reader of `settings.frameless` in src/ or ui/src outside settings.rs, chrome_host/settings.rs, settings_panel.rs | contract only | NOTHING | marked notYet; label promises auto-hide. Owned by T11 |
| bookmarksBar (bookmarks_bar) | Bookmarks bar | none. No bookmarks bar exists in src/ or ui/src | contract only | NOTHING | marked notYet; the GDI panel cycles the value too |
| bookmarksBarTabGroups (bookmarks_bar_tab_groups) | Show tab groups in the bookmarks bar | none | contract only | NOTHING | marked notYet |
| sidePanelPosition (side_panel_position) | Side panel position | `win32/side_panel.rs:205` reads settings.txt at panel open; `panel_left`, `reserved_viewport` | none for position | PARTIAL | acts, but at the next panel open from the file (not via the snapshot or `apply_live_settings`). Page says "Applies after restart", which is too cautious. Only the native side-panel host reads it |
| fontSize (font_size) | Font size | `window.rs:1010` `surface.set_font`; `ViewState.font_scale` -> HTML; `refresh_edit_font` | protocol.rs serialisation only | WORKS | label says "Bowser's own pages"; it scales the chrome |
| uiFont (ui_font) | Customise fonts | same path; `ViewState.ui_font` (protocol.rs:1445) | protocol.rs:3222 | WORKS | |
| defaultZoom (default_zoom) | Default zoom | `app.rs:3729-3745` -> `site_prefs::zoom_for_uri(url, default)` | Rust site_prefs.rs:465 | WORKS | applies on navigation; the row says so |
| showZoomIndicator (show_zoom_indicator) | Show the zoom indicator | `window.rs:8181/9032` -> `ViewState.show_zoom` | none found | WORKS | |
| translateOffer (translate_offer) | Offer to translate pages | `app/page_translate.rs::translate_offer` (own command, `App::set_translate_offer`) | Rust app/tests/translate_flow.rs:390-399 | WORKS | |
| translateAuto (translate_auto) | Translate automatically on sites set to always translate | `page_translate.rs:172/592` | translate_flow.rs:425, translate/mod.rs:568 | WORKS | |
| translateTarget (translate_target) | Translate into | `page_translate.rs:196/238` | translate_flow.rs:88/195/408 | WORKS | |
| shiftRightClickContext (shift_right_click_context) | Shift+Right Click shows the system menu | `window.rs::system_menu_requested` (3376, 9171, 9218, 9626) | none found | MISMATCH | works, but row still carries `notYet` |
| ctrlShiftCCopyLink (ctrl_shift_c_copy_link) | Ctrl+Shift+C copies the address | `app.rs::gated_chords` -> `shortcuts.rs` `Command::CopyLink` -> `window.rs:10679 copy_active_tab_link` | Rust host.rs:1692 | MISMATCH | works, but row still carries `notYet` |
| blocker (blocker_enabled) | Block ads and trackers | `app.rs:4750` (blocker built at startup from settings); `blocker.rs` | Rust blocker.rs:1746 | WORKS | startup only, not in `apply_live_settings`; matches "restart" |
| blockYoutubeVideoAds (block_youtube_video_ads) | Block YouTube video ads | `blocker.rs:1175` -> `youtube::video_ads_blocked_in_file` (reads the file per navigation) | Rust blocker.rs:2247, youtube.rs tests | WORKS | live via the file |
| allow (blocker_allow) | Sites the blocker skips | `app.rs:4750` -> `BlockerState::load(.., blocker_allow)`, `blocker.rs::site_allowed` | Rust blocker.rs:1746 | WORKS | startup; "restart" |
| tracking (tracking_prevention) | Tracking prevention | `app.rs::tracking_prevention_request` at engine creation | Rust app.rs:7251 | WORKS | startup; "restart" |
| history (history file `history_enabled`, not settings.txt) | Save browsing history | `profile.rs::history_enabled`; gates `window.rs:743/6588/10694`, `panel_history.rs` | Rust profile.rs:1338 | WORKS | own command (`Action::History`), cached per profile object |
| dashboard | Privacy dashboard | `Action::Privacy` -> `privacy_panel::open` | ui/test/panel-privacy.mjs | WORKS | action row |
| efficiency (efficiency_mode) | Efficiency mode | `app.rs::apply_efficiency_mode`, `refresh_efficiency`, `efficiency.rs::efficiency_active` | Rust efficiency.rs:209, app.rs:7244/7475/7529 | WORKS | live |
| sleep (sleep_enabled) | Sleep hidden tabs | `app.rs::apply_sleep_settings`, `efficiency.rs::background_policy` | Rust app.rs:7453/7553/7584 | WORKS | |
| sleepAfter (sleep_after_secs) | Sleep after | same | Rust app.rs:7453, settings.rs:1147 | WORKS | |
| engine (search_engine) | Search engine | `app.rs:5213` re-reads settings.txt by file stamp -> `SearchProvider::from_config` | Rust app.rs:7126-7131, settings.rs:1263-1333, navigation.rs | WORKS | live via the file |
| downloadDir (download_dir) | Location | `app.rs:3918` -> `downloads::resolve_dir`; picker `pick_download_folder` | Rust downloads/tests.rs:202-211, settings.rs:1407 | WORKS | own command |
| downloadAsk (ask_download_location) | Ask where to save each file | `app.rs:3923`, `set_ask_download_location` | settings.rs:1407 (round-trip) | WORKS | no behaviour test found |
| media | Media support | `media` snapshot section (read-only probe) | ui/test/panel-media.mjs | WORKS | read-only |
| extensions | Installed extensions | `settings_page.rs::run_extension`, `chrome_host/extensions_page.rs` | ui/test/extensions.mjs | WORKS | |
| autoUpdate | Update automatically | `updater::set_auto_update` (install-wide, own command) | Rust updater.rs tests | WORKS | |
| updateStatus | Bowser updates | `Action::CheckUpdates / RestartUpdate` | Rust updater tests, UI `updateView` | WORKS | |
| version | Version | `snapshot_json` | UI | WORKS | display |
| file | Settings file | `Action::OpenFile` -> `settings_panel::launch_settings_file` | none | WORKS | |
| reset | Reset settings | `Action::Reset` (defaults saved and applied live) | none for the action | WORKS | |
| defaultBrowser | Default browser | `Action::SetDefault` -> `default_browser::make_default` | Rust default_browser.rs tests | WORKS | |

Non-row stored keys, not on the page: `sidebar_collapsed`, `side_panel_width` (persisted by drag or collapse), and `guest`.

## Non-WORKS rows (one-line reason each)

- toolbarButtons: PARTIAL. Only `site-info` and `split` are read, and only by the GDI-painted omnibox. The other six ids and the whole HTML toolbar ignore it.
- sidePanelPosition: PARTIAL. The value is read from settings.txt each time the native side panel opens. It is not in the snapshot or live path, and the "restart" note is wrong.
- frameless: NOTHING. No consumer exists anywhere. The label promises auto-hide, which does not exist. T11 owns it.
- bookmarksBar: NOTHING. There is no bookmarks bar.
- bookmarksBarTabGroups: NOTHING. It depends on the same missing bar.
- pinNewTabGroups: NOTHING. It depends on the same missing bar.
- tabPreviewImages: NOTHING. There is no tab preview card.
- tabPreviewMemory: NOTHING. There is no tab preview card.
- ctrlSVerticalTabs: MISMATCH. The code works (shortcuts.rs:235 -> `toggle_sidebar`), but the page still says "Has no effect yet".
- shiftRightClickContext: MISMATCH. The code works (window.rs `system_menu_requested`), but the page still says "Has no effect yet".
- ctrlShiftCCopyLink: MISMATCH. The code works (`Command::CopyLink`), but the page still says "Has no effect yet".
- showHomeButton: label and description say "toolbar", behaviour is the rail Home button. Minor wording drift, not graded as MISMATCH.
- `ui/test/settings-page.mjs:166` hard-codes the notYet list `[bookmarksBar, bookmarksBarTabGroups, ctrlSVerticalTabs, ctrlShiftCCopyLink, frameless, pinNewTabGroups, shiftRightClickContext, tabPreviewImages, tabPreviewMemory]`. Fixing the 3 MISMATCH rows means editing that test.
- Rows with no behaviour test found: showHomeButton, minimalUrl, showZoomIndicator, splitEdgeDrop, shiftRightClickContext, downloadAsk, sidePanelPosition, roundedContentFrame (off state), fontSize/uiFont (serialisation only).

## Rows missing from the GDI fallback (`src/win32/settings_panel.rs`) or from the HTML page

GDI fallback has controls for: sleep, sleep secs, blocker, allow list, search engine, tab groups, layout, sidebar width, theme, accent, font size, UI font, bookmarks bar, side panel position, centred bar, minimal URL, load progress, frameless, rounded frame, home button, bookmarks-bar tab groups, tab preview images and memory, the 8 toolbar buttons, default zoom, zoom indicator, new tab beside, Ctrl+Tab MRU, shift-right-click, Ctrl+Shift+C, Ctrl+S, pin groups, split edge drop, YouTube ads, default browser, open file, reset.

Missing from the GDI fallback (the form preserves their values on save, see settings_panel.rs ~1837-1870):
- `efficiency_mode`
- `tracking_prevention`
- `history`
- `autoUpdate`
- update status and version
- `download_dir` and `ask_download_location` (the code comment says so)
- `translate_offer`, `translate_auto`, `translate_target`
- profiles, import, privacy dashboard
- extensions, media

Missing from the HTML page: none. Every setting key has a row. Not on either page: `sidebar_collapsed` and `side_panel_width`, which are persisted state, not options.

## Vivaldi-port features already on origin/main

- Undo close tab: PARTIAL. `recently_closed` (8 entries, http/https, non-private) in `src/app.rs:682/2745`, shown as "Recently closed" in the main menu (`src/win32/window.rs:6068`) and on the empty split side (`chrome_host/protocol.rs:306/360`, `reopen n`). No Ctrl+Shift+T: `src/shortcuts.rs:364` tests that chord as unbound.
- Sessions: PARTIAL. Named URL snapshots in `src/productivity.rs` (`sessions.txt`, `SavedSession`, `parse/serialize_sessions`), UI via `show_sessions` in `src/win32/window.rs:6896` (save, restore, delete) and the palette. No selected-tabs save. Separately, `src/session.rs` does crash recovery.
- Speed dials: NONE. No speed-dial or start-page code (grep).
- Layout presets: NONE. Only the 4 `tab_layout` values (`src/settings.rs`, `chrome/layout.rs`), no presets or preset thumbnails.
- Tab stacks: PARTIAL, different feature. Automatic domain "islands": `src/tab_groups.rs`, `src/tab_organisation.rs`, `chrome/group_ui.rs`, `chrome/island_menu.rs`. They have names, colours and collapse, but no stack styles, accordion or stack actions.
- Tiling: PARTIAL. Split view with up to 4 panes, `src/split_view.rs`, `win32/split_ui.rs`, `ui/src/split*.ts`, edge drop. No tiling commands (Ctrl+F7/F8/F9), grid, or untile.
- Workspaces: EXISTS. `src/workspaces.rs` (icon, colour, per-tab membership, `workspaces.txt`), rail entries in `win32/window.rs`. No auto-open rules, no switch-by-scroll.
- Quick commands: PARTIAL. Command palette Ctrl+Shift+K: `src/win32/command_palette.rs`, `ui/src/palette.ts`, `palette-logic.ts` (sections tab, command, bookmark, history), `productivity::CommandRegistry`. No prefixes (`bookmark:` etc.), no calculator.
- Notes: EXISTS. `src/win32/notes_panel.rs` (notes and tasks sidebar, `notes.txt`), `ui/src/panels/*`, `ui/test/panel-notes.mjs`, `ToggleNotes` command. No markdown, folders, export or import found.
- Capture: NONE.
- Reader view: EXISTS. `src/reader/mod.rs`, `reader.js`, `reader.css`, `bar.css`, `docs/reader.md`, Ctrl+Alt+R. Prefs are theme and size only. No font face, line height, column width, vertical text.
- Themes: PARTIAL. `src/theme.rs`: Helium or System plus an accent override, contrast tests. No colour set, schedule, or import/export.
- Adblock levels: PARTIAL. `src/blocker.rs`, `src/blocker/youtube.rs`: on/off, allow list, YouTube video ads, per-site toggle Ctrl+Shift+B, and tracking prevention balanced or strict (`settings.rs`). No none/trackers/trackers+ads levels, no cookie-warning removal, no source management.
- Feeds: NONE (grep hits were incidental).
- Mouse gestures: NONE.
- Auto-hide: NONE. Only the dead `frameless` setting. No pointer-edge reveal code.

Also present, outside the list: private tabs (`src/private.rs`), translate (`src/translate`, `src/app/page_translate.rs`), PiP (`src/pip.rs`), passwords (`src/passwords`), DevTools dock, extensions (`src/crx*.rs`), sidebar rail panels (history, bookmarks, customize, privacy).

## Caveats

- Everything above is static reading of origin/main. Nothing was built, run or launched.
- "No test found" means a grep of `src/` and `ui/test/` found no test naming the key or consumer. The `tests/` directory only has fixtures.
- With `html-chrome-default` (release), the HTML toolbar ignores `toolbar_buttons`. This was checked by absence (no reference in `ui/src/view.ts`, `main.ts` or `ViewState`).
