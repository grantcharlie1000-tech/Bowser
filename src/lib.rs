pub mod agent;
pub mod app;
pub mod blocker;
pub mod bookmark_extras;
pub mod bookmarks;
pub mod address_input;
pub mod address_runtime;
pub mod address_settings;
pub mod address_suggest;
pub mod calc;
pub mod chrome;
pub mod chrome_host;
pub mod commands;
pub mod crx;
pub mod crx_fetch;
pub mod crx_file;
pub mod crx_unpack;
pub mod default_browser;
pub mod download_shell;
pub mod downloads;
pub mod efficiency;
pub mod engine;
pub mod extensions;
pub mod favicon;
pub mod feeds; // the feed reader: reading, finding, storing and fetching feeds (T13)
pub mod import;
pub mod internal_page;
pub mod navigation;
pub mod omnibox;
pub mod page_tools; // capture, spatial navigation, reader defaults, zoom scope, pop-out video (T7)
pub mod capture; // page capture: CDP requests, image stitching, files, clipboard (T7)
pub mod notes_book; // notes: folders, sort, markdown, export and import (T7)
pub mod passwords;
pub mod performance;
pub mod permissions;
pub mod pip;
pub mod privacy_stats;
pub mod private;
pub mod productivity;
pub mod profile;
pub mod reader;
pub mod translate;
pub mod search_engines;
pub mod session;
pub mod settings;
pub mod settings_privacy; // blocker level, history retention, website appearance, private theme (T3)
pub mod browsing_data; // Delete Browsing Data: kinds, ranges and the plan (T3)
pub mod history_retention; // drops visits older than the retention setting (T3)
pub mod website_look; // the colour scheme sites are told, and forced dark mode (T3)
pub mod layout_prefs; // bar positions, presets, density, UI zoom (T6)
pub mod window_prefs; // window appearance and panel options (T9)
pub mod panel_prefs; // per-web-panel zoom, mute, reload, width, notifications and removed panels (T9)
pub mod shortcuts;
pub mod sidebar;
pub mod site_prefs;
pub mod site_security;
pub mod split_view;
pub mod auto_hide; // auto-hide of the toolbars, tabs and panels, and full screen (docs/design/t11-auto-hide.md)
pub mod start_page; // the new-tab start page: speed dials, widgets, startup (docs/design/start-page.md)
pub mod drop_intent; // when a dragged tab over the page means "split" (docs/design/tab-drag.md)
pub mod tab_groups;
pub mod tab_options;
pub mod tab_state;
pub mod thumbnail;
pub mod tab_organisation;
pub mod tabs;
pub mod theme;
pub mod theme_custom; // colours, accent mode, light/dark schedule, theme file (T8)
pub mod toolbar_prefs; // the address, tab and panel bars' buttons and the feed settings (T13)
pub mod updater;
pub mod vertical_tabs;
pub mod win32;
pub mod workspaces;
pub mod stacks; // tab stacks, tiling and workspace rules: the pure rules (T10)
pub mod workspace_rules; // sites that open in a named workspace (T10)
