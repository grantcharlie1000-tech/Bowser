# Bowser settings audit, wave-1 integration (vivaldi/wave1-integration @ 8808ffc0, read-only)

Method. Static reading only: nothing was built, run or launched, and no git state was changed. The row list was confirmed against `ui/src/settings-page-logic.ts` (SECTIONS, ROWS), `ui/src/tab-options-logic.ts` (TAB_OPTIONS), `ui/src/address-settings-logic.ts` (ADDRESS_ROWS) and the keyboard/commands rows: 164 rows in 16 sections. `newTabBesideActive` is gone (Tabs > New tab position writes the same `new_tab_beside_active` key), `ctrlTabMru`, `tabPreviewImages` and `tabPreviewMemory` sit in Tabs, and `qcNickname` is new. Only `frameless` still carries `notYet` (`ui/test/settings-page.mjs:167` asserts exactly that). For each row I followed the chain UI control -> message (`set` save / `tabopt` / `addrset` / `engine` / privacy panel command / `kboption`-style command page actions) -> `src/chrome_host/settings.rs`, `address_page.rs`, `commands_page.rs` or `panels/protection.rs` `map_command` -> `src/win32/settings_page.rs::run` / `run_address` / `run_commands` or `win32/protection.rs::run` -> save -> `App::apply_live_settings` / `apply_privacy_settings` / `set_tab_option` / `commands::save_and_install` -> the consumer that changes behaviour. Unchanged rows reuse the prior pre-thread audit after a spot check. The test column names tests that exercise the consumer. `ui/test/settings-page.mjs` only covers parsing, `saveText` and the row index, so it is not listed per row.

One cross-cutting defect matters for several rows. `App::save_settings` (src/app.rs:1149) writes the whole in-memory `Settings`. Six fields are never copied into memory after a page save: `automatic_tab_grouping`, `tracking_prevention`, `block_youtube_video_ads`, `side_panel_position`, `frameless`, and the legacy `search_engine`. `side_panel_width` is re-read from the file. So any later `save_settings` call puts these six back to their startup values in settings.txt. The callers are: a Tabs option change (`tab_features.rs:86`), translate offer/auto/target, download folder or ask (`app.rs:4141-4169`), a sidebar width drag (`window.rs:4651`) and a sidebar collapse or Ctrl+S (`window.rs:11256`). This is why grouping, tracking and YouTube ads are PARTIAL below.

## profile (You and Bowser)

| key | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| profile (snapshot) | Profile | `chrome_host/settings.rs::snapshot_json` (name, avatar, guest) | UI only | WORKS | display only |
| profiles (action) | Manage profiles | `settings_page.rs::run(Action::Profiles)` -> `window.rs::show_profile_menu_from_page` | none | WORKS | action row |
| import (action) | Import from another browser | `settings_page.rs::run(Action::Import)` -> `import_panel::open` | none in this chain | WORKS | action row |

## appearance

| key | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| tab_layout | Browser layout | `App::apply_tab_layout_settings` (live), `chrome/layout.rs::compute_in_frame_for_with` | `chrome/layout.rs` layout tests, settings.rs round-trip | WORKS | 4 layouts |
| sidebar_width | Sidebar width | `App::apply_tab_layout_settings`, `Settings::effective_sidebar_width` | settings.rs clamp tests | WORKS | vertical layout only |
| theme | Theme | `apply_live_settings` -> `App::settings_accent` -> `theme::resolved_accent` | theme.rs contrast/accent tests | WORKS | Helium or System. A full theme editor is T8 |
| theme_accent | Accent colour | same, plus `chrome_host/protocol.rs` theme tokens | theme.rs, protocol.rs tests | WORKS | |
| website_appearance | Website appearance | privacy panel cmd `webappearance` -> `win32/protection.rs::run` -> `App::apply_privacy_settings` -> `website_look::set` -> `engine/webview2.rs::apply_website_look` (`SetPreferredColorScheme`) | `website_look.rs::the_settings_choose_the_look_and_a_change_is_reported_once`, `panels/protection.rs` map tests | WORKS | live |
| force_dark_websites | Force dark theme on all websites | same path -> `webview2.rs::refresh_force_dark` | `website_look.rs::forcing_dark_sends_the_enable_call_and_not_forcing_clears_the_override` | WORKS | applies to the pages opened next, as the row says |
| private_theme | Private tab theme | `apply_privacy_settings` -> `window.rs` palette pick (4781, 8673 `PrivateTheme::appearance`) | `settings_privacy.rs::the_private_theme_only_changes_the_look_while_a_private_tab_is_shown` | WORKS | |
| toolbar_buttons | Customise your toolbar | `window.rs::PaintPrefs::read` (`site-info`, `split` only) -> `chrome/paint.rs` | settings.rs parse only | PARTIAL | Unchanged by the threads. Only 2 of 8 ids are read, and only by the GDI-painted omnibox. `back`, `forward`, `reload`, `bookmark`, `newtab`, `reader` are never read. The HTML toolbar (`ui/src/view.ts`, `ViewState`) ignores all of them. Owner T13 |
| show_home_button | Show the Home button | `window.rs:1321` adds `RailKey::Home`; Home opens `start.home_target()` (window.rs:5004) or the start page | none | WORKS | The button is on the rail. The row says "toolbar" (wording drift) |
| centred_address_bar | Centre the address bar | `window.rs` layout -> `layout::compute_in_frame_for_with` | layout.rs centred test | WORKS | |
| minimal_url | Show a simplified address while unfocused | `omnibox.rs::address_display`, `win32::sync_address_input` | none names it | WORKS | |
| show_load_progress | Show a loading progress bar | `window.rs:8697/9623` -> `ViewState.show_progress` -> `ui/src/view.ts` | protocol.rs serialisation only | WORKS | |
| automatic_tab_grouping | Group tabs by site | `app.rs:2536` -> `tab_organisation::apply_membership` at startup | app.rs grouping test (~8814) | WORKS | Acts at startup, matching the "restart" flag. Never copied into memory, so a later `App::save_settings` (any Tabs option, translate, download, sidebar drag or collapse) writes the old value back before the restart  Fixed after the audit: `save_settings` keeps the file-only keys and `apply_live_settings` applies the Tabs options (tab_flow tests). |
| split_edge_drop | Allow dropping a dragged tab on the window edge to split | `win32/split_ui.rs:763` | none | WORKS | |
| pin_new_tab_groups | Pin new tab groups to the bookmarks bar | `win32/bookmark_bar.rs::pin_new_groups` -> `save_pinned` (`bar_groups`) | none direct | WORKS | Was NOTHING. The bar now exists. Needs the bookmarks bar shown with tab groups on |
| ctrl_s_vertical_tabs | Ctrl+S collapses or expands the sidebar in the vertical layout | `app.rs` gated chords -> `shortcuts.rs:115` `toggle_sidebar` -> `window.rs::toggle_sidebar` | `commands/mod.rs::gated_chords_are_unbound_until_their_condition_holds` | WORKS | notYet flag removed. Prior MISMATCH fixed |
| rounded_content_frame | Rounded corners around the page | `window.rs::PaintPrefs` -> `paint.rs` `content_rounded` | pixel tests (on state only) | WORKS | |
| frameless | Frameless window | none: read only by settings.rs, chrome_host/settings.rs, settings_panel.rs | contract only | NOTHING | marked notYet. No auto-hide code. Owner T11 |
| bookmarks_bar | Bookmarks bar | `bookmark_bar.rs::wanted` (never/always/newtab) -> `reserve_px` -> `window.rs` relayout, `bookmark_bar::sync` | `bookmark_bar.rs::the_bar_takes_its_height_from_the_top_or_the_bottom_of_the_page`, ui/test/address.mjs bar tests | WORKS | Was NOTHING. Shown only with the HTML chrome (`html_ui_enabled`); the GDI chrome has no bar |
| bookmarks_bar_tab_groups | Show tab groups in the bookmarks bar | `bookmark_bar.rs::group_chips` (310) | none direct | WORKS | was NOTHING |
| bm_bar_folder (addrset) | Bookmarks bar folder | `bookmark_bar.rs::bar_items(.., bar_folder)` | `bookmark_bar.rs::the_bar_shows_the_chosen_folder_or_the_top_level` | WORKS | |
| bm_bar_position (addrset) | Bookmarks bar position | `bookmark_bar.rs::reserve_px` (top or bottom), `reserved_viewport` | `bookmark_bar.rs::the_bar_takes_its_height_from_the_top_or_the_bottom_of_the_page` | WORKS | |
| bm_bar_display (addrset) | Bookmarks bar display | `bookmark_bar.rs::snapshot_for` -> `ui/src/bookmark-bar.ts` | ui/test/address.mjs "each display mode shows what its name says..." | WORKS | |
| side_panel_position | Side panel position | `win32/side_panel.rs:205` reads settings.txt at each panel open | none | PARTIAL | Acts at the next panel open, not after a restart, so the "restart" note is wrong. Never copied into memory, so a later `App::save_settings` reverts the file. Owner T6 layout |
| font_size | Font size | `apply_live_settings` -> `refresh_edit_font`, `ViewState.font_scale` | protocol.rs serialisation | WORKS | it scales the chrome |
| ui_font | Customise fonts | same; `ViewState.ui_font` | protocol.rs serialisation | WORKS | |
| default_zoom | Default zoom | `site_prefs::zoom_for_uri` on navigation | `site_prefs.rs` zoom tests | WORKS | |
| show_zoom_indicator | Show the zoom indicator | `window.rs:8702/9628` -> `ViewState.show_zoom` -> `view.ts:530` | ui/test/zoom.mjs | WORKS | |
| translate_offer | Offer to translate pages in other languages | own cmd -> `App::set_translate_offer` -> `app/page_translate.rs` | `app/tests/translate_flow.rs` | WORKS | |
| translate_auto | Translate automatically on sites set to always translate | `App::set_translate_auto` -> `page_translate.rs` | translate_flow.rs, translate/mod.rs | WORKS | |
| translate_target | Translate into | `App::set_translate_target` -> `page_translate.rs` | translate_flow.rs | WORKS | |
| shift_right_click_context | Shift+Right Click shows the system menu | `App::shift_right_click_context` -> `window.rs::system_menu_wanted` | window.rs default test (11717) | WORKS | notYet flag removed. Prior MISMATCH fixed |
| ctrl_shift_c_copy_link | Ctrl+Shift+C copies the address | gated registry binding -> `window.rs::copy_active_tab_link` | `commands/mod.rs::ctrl_shift_c_is_a_registry_binding_that_moves_with_one_assignment` | WORKS | notYet flag removed. Prior MISMATCH fixed |

## tabs

All `tabs_*` rows: page `tabopt` command -> `settings_page.rs::run(Action::TabOption)` -> `App::set_tab_option` (`app/tab_features.rs`), which saves and applies live. The strip options go to the HTML strip through `window.rs::html_strip_options` -> `protocol.rs::tabopts_json` -> `ui/src/tabs.ts`. Rust tests are in `src/app/tests/tab_flow.rs` (TF) and `src/tab_options.rs` (TO). UI tests are in `ui/test/tab-options.mjs` and `tab-options-dom.mjs` (TOD).

| key | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| tabs_new_tab_page | New tab page | `App::open_new_tab` -> `tab_options::new_tab_address(.., start.home_target())`; `start_ui::enabled` -> `start_page_shown` | TF `the_new_tab_page_option_decides_what_a_fresh_tab_opens`, `the_new_tab_page_homepage_and_blank_choices_use_the_start_page_settings` | WORKS | start, homepage, blank and url are all handled |
| tabs_new_tab_url | Address of the new tab page | `new_tab_address` (Url choice) | TF `the_new_tab_page_option_decides_what_a_fresh_tab_opens`, settings.rs script-URL refusal | WORKS | |
| tabs_new_tab_position | New tab position | `tab_options::apply_assignment` -> `new_tab_beside_active` + `tabs.new_tab_related`; `tab_features.rs:12` | TF `a_new_tab_goes_where_the_position_option_says_in_the_strip`, TO position test | WORKS | owns the old `new_tab_beside_active` key |
| tabs_link_background | Open links in new tabs in the background | `App::open_link_tab` | TF `a_link_in_a_new_tab_can_load_in_the_background` | WORKS | |
| tabs_duplicate_position | Duplicated tab position | `window.rs:5622` duplicate handler | none (TF only names the enum) | WORKS | no behaviour test |
| tabs_focus_page | Focus the page on a new tab | `window.rs:7181` -> `App::focus_page` | none | WORKS | no behaviour test |
| tabs_close_related | Return to the related tab when closing | `tab_options::close_target` via `tab_features.rs:48` | TF `closing_the_active_tab_activates_the_tab_the_option_names` | WORKS | |
| tabs_close_activation | Tab to activate after closing | `close_target` (right, left, recent) | same TF test | WORKS | |
| tabs_align_close_button | Keep the next close button under the pointer | strip `alignClose` -> `tabs.ts` width lock | TOD `closing by mouse holds the tab width until the pointer leaves, when asked` | WORKS | HTML strip only |
| tabs_keep_window | Keep the window open when the last tab closes | `tab_options::close_kind` (tab_features.rs:363) | TF `a_user_close_follows_the_pinned_and_last_tab_options` | WORKS | |
| tabs_confirm_close | Confirm when closing many tabs | `App::close_needs_confirmation` -> `window.rs:7207` `ask_yes_no` | TF `closing_many_tabs_asks_only_from_the_threshold_up` | WORKS | |
| tabs_double_click | Double click on a tab | `window.rs:9770` (ignore, close, rename) | TOD `a double click on a tab goes to Rust...` | WORKS | |
| tabs_close_left | Close button on the left side | strip `closeLeft` -> `tabs.ts` | TOD `the strip follows the host: classes...` | WORKS | HTML strip only |
| tabs_close_always | Always show the close button | strip `closeAlways` | TOD strip classes test | WORKS | HTML strip only |
| tabs_scroll | Scroll the tab bar horizontally | strip `scroll` | tab-options.mjs `the width lock and the strip classes follow the options` | WORKS | HTML strip only |
| tabs_remove_spacing_maximised | Remove tab spacing when maximised | strip `removeSpacing` + `maximised` | same | WORKS | HTML strip only |
| tabs_active_min_width | Active tab minimum width | strip `minWidth` -> `--tab-active-min` (tabs.ts:406) | protocol.rs tabopts test | WORKS | |
| tabs_load_indicator | Page load progress | strip `indicator` -> `tabs.ts::loadMark` | TOD `page load shows as a spinner, a bar, a favicon ring or nothing...` | WORKS | all 4 choices |
| tabs_wheel_switch | Switch tabs by scrolling | strip `wheel` | TOD `the wheel switches tabs only when asked...` | WORKS | |
| tabs_minimise_active | Minimise the active tab | `window.rs:9761` -> `App::minimise_active_tab` | TF `minimise_active_goes_back_to_the_last_used_tab` | WORKS | |
| tabs_title_notifications | Detect page title notifications | `app.rs:5758`, `tab_features.rs:54/445` | TF `unread_marks_come_from_background_loads_and_growing_counts...` | WORKS | |
| tabs_unread_indicator | Show unread indicators | `tab_features.rs:444` | same TF test | WORKS | |
| tabs_dim_hibernated | Dim the icon of hibernated tabs | `tab_features.rs:446` | TF `the_strip_dims_hibernated_tabs_and_marks_idle_ones_only_when_asked` | WORKS | |
| tabs_inactive_days | Inactive tabs threshold | `tab_options::is_inactive` (tab_features.rs:451) | same TF test | WORKS | |
| ctrl_tab_mru | Cycle tabs most-recently-used first with Ctrl+Tab | `App::select_next/select_prev/select_mru_other` (app.rs:1980) | `app.rs::ctrl_tab_mru_swaps_to_the_last_active_tab_and_can_be_turned_off` | WORKS | moved from appearance |
| tab_preview_images | Show a thumbnail in tab previews | `apply_live_settings` -> `tab_features.rs:520-560` thumbnail capture -> hover card | TOD `the hover card shows a thumbnail and memory only when Rust sent them`, `the tab preview rows act now` | WORKS | Was NOTHING. Private tabs are excluded |
| tab_preview_memory | Show memory use in tab previews | `tab_features.rs:529/622` memory figure | same TOD tests | WORKS | was NOTHING |
| tabs_selection | Select tabs with Ctrl and Shift | `tab_features.rs:461`, strip `selection` | TF `the_selection_is_what_a_tab_action_applies_to`, TOD `with selection off a Ctrl click is just a click` | WORKS | |
| tabs_selection_active | Include the active tab in the selection | `tab_features.rs:469/479` | same TF test | WORKS | |
| tabs_mute_mode | Tab audio | `App::apply_mute_mode`, `tab_options::tab_may_play` | TF `the_audio_rule_mutes_background_tabs...`, `prioritise_mutes_the_others_only_while_the_active_tab_plays` | WORKS | all 3 choices |
| tabs_pinned_close | Closing a pinned tab | `close_kind`, `window.rs:7202` | TF `a_user_close_follows_the_pinned_and_last_tab_options`, `minimising_a_pinned_tab_moves_on_and_puts_it_to_sleep` | WORKS | |
| tabs_pinned_separator | Separator below pinned tabs | strip `separator` | TOD strip test | WORKS | HTML strip only |
| tabs_pinned_restrict | Keep pinned tabs on their site | `tab_options::leaves_pinned_site` (tab_features.rs:690) | TF `a_pinned_tab_sends_links_to_other_sites_to_a_new_tab_when_asked` | WORKS | |
| tabs_lazy_restore | Load restored tabs when first selected | `app.rs:5054` session restore | TF `restored_tabs_load_lazily_unless_the_options_say_otherwise` | WORKS | next restore |
| tabs_load_pinned | Always load pinned tabs | `app.rs:5054` | same TF test, `eager_restored_tabs_load_once_the_engine_can` | WORKS | |

## startpage

Start rows ride the main `set` save (`startSaveLines`) -> `apply_live_settings` (`state.settings.start`) -> `start_ui::settings_changed` -> `chrome_host/start_page.rs` settings object -> `ui/src/start-page.ts`. Tests: `src/start_page/tests.rs` (SP) and `ui/test/start-page.mjs` (SPU).

| key | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| start_group_reopen | Open the start page on | `start_page/service.rs:100` `dial_view(.., group_reopen, ..)` | SP `the_page_opens_on_the_last_or_the_first_group_as_set` | WORKS | |
| start_titles | Show titles | `start-page.ts:482/746` `titleState` | SPU `titles always, never and when needed: the tile says which` | WORKS | |
| start_add_button | Show the add button | `start-page.ts` add tile | SPU `the add button shows or hides with its setting...` | WORKS | |
| start_columns | Maximum columns | `start-page.ts:790` `columnsFor` | SPU `columns: the setting is a ceiling...`, `maximum columns, the five sizes and drag...` | WORKS | |
| start_thumb_size | Tile size | `start-page.ts:791` `tileWidth` | same SPU test | WORKS | |
| start_drag_reorder | Drag to reorder | `start-page.ts` drag handlers gated by `dragReorder` | SPU `drag to reorder is a setting...` | WORKS | |
| homepage | Homepage | `StartSettings::home_target` -> Home button (window.rs:5004), new tab (`new_tab_address`), startup | SP `startup_modes_decide_what_the_first_window_opens`, TF homepage test | WORKS | one key shared with Tabs > New tab page |
| startup | When Bowser starts | `start_page::startup_plan` -> `App::apply_startup_plan` (window.rs:2077); last session via `app.rs:4977` `restore_clean` | SP `startup_modes_decide_what_the_first_window_opens` | WORKS | next launch, by nature |
| startup_pages | Pages to open at startup | `startup_plan` (SpecificPages) | same SP test | WORKS | |
| start_widgets | Show the dashboard | `start-page.ts:807` | SPU `widgets: each kind draws, and the dashboard follows its settings` | WORKS | |
| start_widget_columns | Widget columns | `start-page.ts:792` | same SPU test | WORKS | |
| start_widget_height | Widget height | `start-page.ts` | same | WORKS | |
| start_widget_bg | Widget background | `start-page.ts` | same | WORKS | |
| start_widget_color | Custom widget colour | `start-page.ts` | SPU `the custom widget colour gets the text that reads on it` | WORKS | |
| start_widget_drag | Drag to rearrange widgets | `start-page.ts` | SPU `widgets drag by their handle and move with Alt and the arrows; the setting turns both off` | WORKS | |

## privacy

Level, cookie notices, retention and site/list/data commands use the privacy panel family: `chrome_host/panels/protection.rs::map_command` -> `win32/protection.rs::run` -> `update_settings` -> `App::apply_privacy_settings` (live) -> `BlockerState::apply_settings`.

| key | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| blocker_enabled + blocker_level | Blocking level | `blocklevel` -> `apply_privacy_settings` -> `blocker.rs::apply_settings` (none, trackers, ads; generation bump) | `blocker.rs::the_three_global_levels_block_nothing_trackers_or_trackers_and_ads`, `panels/protection.rs` level tests | WORKS | Now a live level (was on/off at startup). The blocker is always built |
| cookie_notices | Hide cookie notices | `blocker.rs::cookie_notices_for` (site choice wins) | `blocker.rs::element_hiding_of_ads_needs_the_full_level_but_cookie_notices_do_not` | WORKS | live |
| block_youtube_video_ads | Block YouTube video ads | `blocker/youtube.rs::video_ads_blocked_in_file` per navigation | `blocker.rs::the_player_rewrite_follows_the_switch_site_exceptions_and_private_tabs`, youtube.rs tests | WORKS | Acts from the file. Never copied into memory, so a later `App::save_settings` silently reverts the file  Fixed after the audit: `save_settings` keeps the file-only keys and `apply_live_settings` applies the Tabs options (tab_flow tests). |
| blocker_allow | Sites the blocker skips | `apply_settings` -> `config_allow`, `site_allowed` | `blocker.rs::allow_list_and_disabled_switch_stop_blocking` | WORKS | now live too |
| tracking_prevention | Tracking prevention | `app.rs::tracking_prevention_request` at engine creation | app.rs tracking test | WORKS | Acts after a restart, matching its flag. Never copied into memory, so a later `App::save_settings` before the restart reverts the choice  Fixed after the audit: `save_settings` keeps the file-only keys and `apply_live_settings` applies the Tabs options (tab_flow tests). |
| history (profile file) | Save browsing history | `Action::History` -> `profile.set_history_enabled`; gates recording | `profile.rs` history test | WORKS | own command |
| history_retention | Keep history for | `history_retention::set_current`, `prune_now` (protection.rs:150), `after_visit`, `finish_at_exit` (session) | `history_retention.rs::each_period_keeps_exactly_the_visits_inside_it`, `session_only_removes_every_visit` | WORKS | live |
| site rules (blocker dir) | Sites with their own choices | `site-level` / `site-cookies` / `site-remove` -> `BlockerState::set_site_level`, `set_site_cookies`, `remove_site_rule` | `blocker.rs::a_site_level_overrides_the_global_one_both_ways_and_the_nearer_site_wins` | WORKS | |
| blocker sources | Blocking lists | `src-toggle/add-url/add-file/update/remove` -> `Registry` -> `rebuild` -> `reload_sources`; fetch via `apply_fetched` | `blocker/sources.rs` tests, protection map tests | WORKS | |
| delete data (action) | Delete browsing data | `DeleteData` -> `browsing_data::plan` -> `App::delete_browsing_data` (passwords need a native yes) | `browsing_data.rs::each_kind_reaches_the_right_worker` and 7 others | WORKS | includes typed history |
| dashboard (action) | Privacy dashboard | `Action::Privacy` -> `privacy_panel::open` | ui/test/panel-privacy.mjs | WORKS | |

## performance

| key | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| efficiency_mode | Efficiency mode | `App::apply_efficiency_mode` -> `refresh_efficiency`, `efficiency.rs` | efficiency.rs, app.rs efficiency tests | WORKS | live |
| sleep_enabled | Sleep hidden tabs | `App::apply_sleep_settings` -> `background_policy` | `app.rs::hidden_tab_sleeps_after_the_configured_delay_without_memory_pressure` | WORKS | |
| sleep_after_secs | Sleep after | same | same | WORKS | |

## address

All are `addrset` commands -> `address_page.rs::map_command` (`AddressSettings::owns`) -> `settings_page.rs::run_address` -> save -> `apply_live_settings` (`state.settings.address`, `omnibox::set_*`). The toolbar flags go to `ui/src/main.ts` through `address_page.rs::addr_json`. Tests: `address_input.rs` (AI), `address_suggest.rs` (AS), `omnibox.rs` (OB) and `ui/test/address.mjs` (AU).

| key | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| address_bar_show | Show the address bar | `addr_json.show` -> `main.ts:425` pill `data-hidden`; command `command_actions.rs:180` | `command_actions.rs::the_address_bar_command_flips_the_saved_switch_and_it_survives_a_reload` | WORKS | HTML toolbar |
| address_autocomplete | Complete addresses as you type | `omnibox::set_autocomplete` | OB `autocomplete_off_offers_no_completion` | WORKS | |
| address_show_full | Show the full address | `omnibox::set_show_full_address` | OB `show_full_address_keeps_scheme_and_www_and_wins_over_the_minimal_form` | WORKS | |
| address_select_on_activate | Select the address when the field is activated | `addr_json.select` -> `main.ts:334-397` | none for DOM | WORKS | |
| address_copy_encoded | Copy and cut the encoded address | `main.ts:246` `clipboardText` | AU `copying or cutting the whole address puts its encoded form...` | WORKS | |
| address_qr_button | Show a QR code button | `main.ts:427` button, `a:'qr'` | AU QR encoder tests | WORKS | |
| dd_show_while_typing | Show the drop-down while typing | `win32/address_bar.rs:212`, `main.ts:197` | none direct | WORKS | |
| dd_always_badge | Always show a badge on drop-down items | `html_suggest.rs:116/131` | AU `badges: always shows one on every row but the default...` | WORKS | |
| dd_typed_history | Show typed history when the field is focused | `main.ts:192/351` `showsTypedHistoryOnFocus` | AU `typed history shows on focus only when enabled...` | WORKS | |
| dd_typed_include_searches | Include searches in typed history | `address_suggest.rs:486` | AS `typed_rows_list_newest_first_and_honour_the_search_option` | WORKS | |
| dd_history_button | Show a typed history button | `main.ts:426` | none | WORKS | |
| dd_max_results | Maximum results | `address_suggest.rs:408`, `address_bar.rs:269` | AS `the_result_limit_applies_to_all_rows` | WORKS | |
| dd_prioritise_bookmarks | Prioritise bookmarks | `address_suggest.rs:436` | AS `prioritising_bookmarks_lifts_them_over_an_earlier_source` | WORKS | |
| dd_prioritise_direct | Prioritise the direct match | `address_suggest.rs:443` | AS `prioritising_the_direct_match_lifts_the_page_whose_host_was_typed` | WORKS | |
| dd_sources | Suggestion sources and order | `AddressSettings::has_source`, source order in `address_suggest.rs` | AS `source_order_is_the_row_order_and_a_disabled_source_gives_no_rows`, `search_history_and_calculator_rows` | WORKS | |
| sec_always_https | Always use HTTPS | `address_input.rs:157` `upgrade` | AI `always_https_upgrades_public_http_and_leaves_local_hosts_alone` | WORKS | |
| sec_strip_js | Remove JavaScript from pasted addresses | `main.ts:236` | AU `a pasted javascript: prefix is removed...` | WORKS | |
| sec_highlight_base | Highlight the base domain | `addr_json.highlight` + `base` -> `view.ts:428` | AU `the base domain is split out of the host...` | WORKS | |
| domain_expansion_suffix | Domain expansion | `address_input.rs:134` `expand_domain` | AI `ctrl_enter_expands_a_single_word_and_the_suffix_is_configurable` | WORKS | |

## search

| key | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| engines file (`engine` cmd) | Search engines | `address_page.rs::parse_engine_op` -> `search_engines.rs` add/update/remove/move/restore -> `address_runtime::engines` | `search_engines.rs::add_update_remove_move_and_the_three_choices`, `restore_defaults_brings_back_built_ins_and_keeps_custom_engines`, AU engine editor | WORKS | |
| default engine | Default search engine | `EngineStore::effective(false)` in `address_input.rs` | AI `plain_input_is_an_address_or_a_default_engine_search` | WORKS | |
| private engine | Private window search engine | `effective(private)` | AI `private_windows_search_with_their_own_engine`, TF `a_private_tab_searches_with_the_private_window_engine` | WORKS | |
| image engine | Image search engine | `app.rs:5380/5406` `image_engine` (context menu) | `search_engines.rs::add_update_remove_move_and_the_three_choices` | WORKS | |
| search_field | Search field | `sfield_json.mode` -> `main.ts:103-106` (off, field, button) | `address_page.rs::toolbar_flags_and_search_field_are_flat_objects`, AU sfield parse | WORKS | HTML toolbar only; the GDI chrome has no search field |
| search_show_nickname | Show nicknames in the engine list | `engine_label(.., show_nickname)` in address_suggest.rs, address_bar.rs, `main.ts:108` | none direct | WORKS | |
| search_show_icon | Show the engine icon | `main.ts:114` | none | WORKS | |
| search_keep_engine | Keep the last selected engine | `address_bar.rs:97/350` (`search_last_engine`) | none | WORKS | |
| search_keep_text | Keep typed text after searching | `main.ts:287` | none | WORKS | |
| search_suggest_field | Show engine suggestions in the search field | `address_bar.rs:248` | none | WORKS | |
| search_new_tab | Always search in a new tab | `address_input.rs:127` | AI `always_search_in_a_new_tab_applies_to_searches_only` | WORKS | |
| search_in_address | Search from the address field | `address_input.rs:161` | AI `search_in_the_address_field_can_be_turned_off_but_nicknames_and_addresses_still_work` | WORKS | |
| search_selection_background | Search for selected text in a background tab | `app.rs:5392` `context_search_run` | none | WORKS | |
| search_suggest_address | Show engine suggestions in the address field | `address_bar.rs:221` (also needs the Search source) | AS `an_engine_nickname_turns_the_default_row_into_that_engine_s_search_and_adds_its_suggestions` | WORKS | |
| nick_bookmarks | Bookmarks scope nickname | `AddressSettings::parse_scope` in address_input.rs:129, address_suggest.rs:413 | AS `scoped_nicknames_show_only_their_own_scope` | WORKS | |
| nick_history | History scope nickname | same | same | WORKS | |
| nick_tabs | Tabs scope nickname | same | same | WORKS | |

## bookmarks

| key | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| bm_open_new_tab | Open bookmarks in a new tab | `panel_bookmarks.rs:1027`, `html_panel.rs:586`, `bookmark_bar.rs:468` | none | WORKS | |
| bm_panel_single_click | Open panel items with a single click | `html_panel.rs:447` snapshot -> `ui/src/panels/bookmarks.ts:203` | ui/test/panel-bookmarks.mjs (snapshot) | WORKS | |
| bm_confirm_open | Ask before opening many bookmarks | `address_bar.rs:395` `confirm_needed` in `open_many` | none direct | WORKS | |
| bm_fetch_metadata | Fetch bookmark details from the page | `address_dialog.rs:418/458` | `address_dialog.rs::page_metadata_is_read_from_the_double_encoded_answer` | WORKS | |
| bm_always_dialog | Always open the bookmark dialog | `address_dialog.rs:452` | none | WORKS | |

## keyboard

Command page actions -> `chrome_host/commands_page.rs` -> `settings_page.rs::run_commands` -> `commands::save_and_install` (live from the next key press).

| key | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| shortcuts_enabled | Keyboard shortcuts | `commands/mod.rs:196/244` (palette chord always on) | `commands/mod.rs::switching_shortcuts_off_leaves_only_the_palette_chord` | WORKS | |
| single_key_shortcuts | Allow single-key shortcuts | `commands/mod.rs:209` | `commands/mod.rs::single_key_shortcuts_need_the_option_a_chrome_source_and_no_typing` | WORKS | |
| alt_opens_menu | Alt opens the menu | `behaviour::alt_tap` <- `window.rs:2536` | `behaviour.rs::alt_on_its_own_opens_the_menu_only_while_the_option_is_on` | WORKS | |
| focus_cycling | F6 moves between the address bar and the page | `behaviour::f6_moves_to_page` <- `window.rs::run_key_command` (FocusOmnibox arm) | `behaviour.rs::f6_moves_to_the_page_only_with_cycling_on_from_the_address_bar` | WORKS | |
| sites (store) | Prioritise website shortcuts on these sites | `engine/webview2.rs:229` page host check | `commands/mod.rs::a_site_that_prioritises_its_shortcuts_keeps_only_the_browser_priority_list`, `command_actions.rs::toggling_a_site_adds_it_and_then_removes_it...` | WORKS | |
| binds / priority (store) | Shortcuts | `commands::save_and_install` -> live keymap; cheat sheet `show_shortcuts_sheet` | `commands/mod.rs::an_assignment_changes_what_the_chord_runs_and_frees_the_old_one`, ui/test/commands.mjs | WORKS | rebinding acts at once |

## commands (Quick commands)

| key | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| palette_types | Result types | `commands/palette.rs:97-133` (order, prefixes, hidden types) | `palette.rs::prefixes_and_their_short_forms_pick_the_types`, `hidden_types_never_appear_even_through_their_prefix`, ui/test/palette.mjs | WORKS | |
| palette_max_results | Visible results | `palette.rs:98`, `command_palette.rs:880` | `palette.rs::the_limit_caps_typed_results_and_prefixed_lists` | WORKS | |
| palette_keep_last | Keep the last typed search | `command_palette.rs:881` -> `ui/src/palette.ts:330` | palette.mjs `the last typed text is kept only when asked...` | WORKS | |
| palette_open_new_tab | Open links in a new tab | `behaviour::link_open` <- `window.rs:6896` | `behaviour.rs::links_open_in_a_new_tab_only_with_the_option_on...` | WORKS | |
| palette_notes_in_panel | Open notes in the Notes panel | `behaviour::note_pick` <- `window.rs:6903` | `behaviour.rs::a_picked_note_opens_the_panel_or_is_copied` | WORKS | |
| palette_open_nickname | Open a bookmark when its nickname is typed | `command_palette.rs:882` (`autonick`) -> `ui/src/palette.ts:269` `nicknameMatch` | `command_palette.rs::bookmark_nicknames_travel_with_their_rows_and_the_option_with_the_page`, palette.mjs `a nickname typed whole matches its bookmark first and...opens it at once` | WORKS | new row |
| chains (store) | Command chains | `commands/chains.rs` plan -> `command_actions::start` (`WM_CHAIN_STEP` = WM_APP+0x5E1) | `chains.rs::a_scheduled_run_fires_every_step_in_order_after_its_delay_and_then_finishes`, `command_actions.rs::the_main_window_messages_of_the_wave_1_threads_are_distinct` | WORKS | `WM_ADDRESS_SUGGEST` is now 0x5E3 and the bookmark bar is 0x5E2, so there is no clash |

## downloads

| key | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| download_dir | Location | `Action::DownloadFolder` -> `App::pick_download_folder` -> `downloads::resolve_dir` | `downloads/tests.rs` dir tests | WORKS | own command |
| ask_download_location | Ask where to save each file before downloading | `App::set_ask_download_location` -> download start | settings.rs round-trip only | WORKS | no behaviour test |

## media

| key | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| media (probe) | Media support | `media` snapshot section | ui/test/panel-media.mjs | WORKS | read-only |

## extensions

| key | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| extensions | Installed extensions | `settings_page.rs::run_extension`, `chrome_host/extensions_page.rs` | ui/test/extensions.mjs, extensions.rs tests | WORKS | |

## updates

| key | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| auto update (install-wide) | Update automatically | `Action::AutoUpdate` -> `updater::set_auto_update` | updater.rs tests | WORKS | |
| update status | Bowser updates | `Action::CheckUpdates / RestartUpdate` | updater.rs tests, UI `updateView` | WORKS | |

## about

| key | label | acting code | test | verdict | note |
|---|---|---|---|---|---|
| version | Version | `snapshot_json` | UI | WORKS | display |
| settings file | Settings file | `Action::OpenFile` -> `settings_panel::launch_settings_file` | none | WORKS | |
| reset | Reset settings | `Action::Reset`: saves `Settings::default()`, removes the engines file, then layout/efficiency/sleep/`apply_live_settings` | tab_flow::applying_the_file_settings_applies_the_tabs_options_live | WORKS | `apply_live_settings` does not copy `settings.tabs`, so the running Tabs options keep their old values until a restart. The next `App::save_settings` then writes those old values back over the reset. The keyboard and quick-command store is not reset; the description does not claim it is Fixed after the audit: `apply_live_settings` now applies the Tabs options. |
| default browser | Default browser | `Action::SetDefault` -> `default_browser::make_default` | default_browser.rs tests | WORKS | |

## Summary

Counts (164 rows) as audited: WORKS 157, PARTIAL 6, NOTHING 1, MISMATCH 0. After the integration fix that followed (commit "Stop later saves reverting file-only settings"), grouping, tracking, YouTube ads and reset are WORKS: WORKS 161, PARTIAL 2, NOTHING 1, MISMATCH 0.

Since the prior audit: the three MISMATCH rows (ctrlSVerticalTabs, shiftRightClickContext, ctrlShiftCCopyLink) lost their notYet flag and now WORK. The bookmarks bar rows (bookmarksBar, bookmarksBarTabGroups, pinNewTabGroups) and the tab preview rows now act. The blocker is a live level.

Non-WORKS rows:
- frameless: NOTHING. It is saved and marked notYet, and nothing reads it. Owner T11 (auto-hide).
- toolbarButtons: PARTIAL. Only `site-info` and `split` are read, and only by the GDI-painted omnibox. The HTML toolbar ignores the setting. Owner T13 (toolbars).
- sidePanelPosition: PARTIAL. It acts at the next panel open, so the "Applies after restart" note is wrong. A later `App::save_settings` reverts it. Owner T6 (layout).
- grouping (automatic_tab_grouping): PARTIAL. It acts at startup as flagged, but a later `App::save_settings` before the restart reverts the choice. No future thread owns it; it is an integration fix in `apply_live_settings`.
- tracking (tracking_prevention): PARTIAL. Same revert-before-restart defect. No owner; integration fix.
- blockYoutubeVideoAds: PARTIAL. It acts from the file, which a later `App::save_settings` silently reverts. No owner; integration fix.
- reset: PARTIAL. The Tabs options are not applied live, and a later `App::save_settings` undoes them. No owner; integration fix (copy `settings.tabs` in `apply_live_settings`, or re-read the file in `save_settings`).

Minor notes, still graded WORKS:
- showHomeButton: the description says "toolbar", but the button is on the rail.
- The strip-only Tabs options (close-button placement, scroll, spacing, separator, wheel, align) act only in the HTML tab strip.
- The bookmarks bar, search field and address-field flags exist only with the HTML chrome.
- No behaviour test found for: duplicate position, focus page, minimal URL, split edge drop, show home button, history button, engine icon, keep engine, keep text, suggest in field, selection search, bm new tab, bm confirm, bm dialog, ask download location, side panel position, reset.

## GDI fallback (src/win32/settings_panel.rs) gaps

- Controls exist only for the older keys: sleep, blocker on/off, allow list, legacy `search_engine`, grouping, layout, sidebar width, theme, accent, fonts, bookmarks bar, side panel position, centred bar, minimal URL, load progress, frameless, rounded frame, home button, bar tab groups, tab previews, the 8 toolbar buttons, zoom, new tab beside, Ctrl+Tab MRU, the 3 gated shortcuts, pin groups, split edge drop, YouTube ads, default browser, open file, reset.
- There are no controls for: every `tabs_*` option, address/search/bookmark keys, start page, blocker level, cookie notices, history retention, site rules, lists, delete data, website appearance, force dark, private theme, efficiency, tracking, history, downloads, translate, keyboard and quick commands, profiles, import, extensions, media, updates. Save keeps the file's values for all of these (settings_panel.rs ~1825-1884), so nothing is lost.
- Semantics drift: the blocker checkbox is still on/off, while the level is kept from the file. The `search_engine` dropdown writes a legacy key that `search_engines.rs` ignores once the engines file exists. `frameless` is offered with no effect. Pin groups and bar tab groups only act with the HTML chrome, which the GDI fallback implies is off.
- A GDI save calls only `apply_tab_layout_settings` and `apply_live_settings`. It does not call `apply_efficiency_mode` or `apply_sleep_settings`, so sleep changes there wait until the next apply.
