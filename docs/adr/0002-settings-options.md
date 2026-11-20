# 0002. Keep Settings option controls aligned with native behavior

## Context

Settings runs in a WebView. The page indexes its own labels and presents controls, while native Rust code owns persisted values, browser behavior, and the wire contract. A control can be a valid preference while the current configuration gives it no opportunity to act.

## Decision

### Validate the wire contract on both sides

Use the address, tab, and theme snapshot values to paint their respective controls. Keep the TypeScript option definitions and Rust settings definitions in sync. The page checks snapshots and constructs command payloads for immediate feedback. Rust validates edits at the command boundary before applying them. Contract tests check the TypeScript definitions against native fixtures and definitions. The address snapshot carries the address preferences under `s` alongside engine and folder data. Tab and theme edits use the `tabopt` and `themeopt` commands with a single `key=value` assignment. [Address snapshot](../../src/chrome_host/address_page.rs), [Settings snapshot](../../src/chrome_host/settings.rs), [tab tests](../../ui/test/tab-options.mjs), [theme tests](../../ui/test/theme-options.mjs)

### Show controls when they can act

Disable an inapplicable control and hide its row in the ordinary view. Settings search can still surface the row, dimmed. As soon as the controlling preference or layout changes, present the row again with its saved value. This keeps the main view compact while preserving discoverability. [Settings design](../design/settings-page.md), [Settings structure](../design/settings-structure.md)

- The typed-searches filter can act when an empty, focused address field or the history button can open typed history. Bookmark priority requires the Bookmarks source. Search-field controls require the field or its button. Suggestions for plain address typing require address search and the Suggestions source. Nickname suggestions depend on the Suggestions source even when ordinary address searches are off. [Address source logic](../../ui/src/address-settings-logic.ts), [native suggestions](../../src/address_suggest.rs), [address field](../../src/win32/address_bar.rs)
- Tab list appearance, pointer, and wheel controls require a drawn tab list. The horizontal strip has its own width, wheel, maximised spacing, and close-button alignment options. The vertical list keeps its row heights and uses its wheel for scrolling. The *Keep tabs wide* preference makes the horizontal strip begin scrolling sooner so titles stay readable. The behavior behind new tabs, closing, audio, pinned rules, restore, and workspaces can still act through shortcuts and menus when the tab list is hidden. [Tab row logic](../../ui/src/tab-options-logic.ts), [strip styles](../../ui/src/styles/tabs.css)
- Focus a new tab's page when opening a configured homepage or specific address. Keep address-field focus when the new tab shows the start page or is blank. Confirm-close thresholds begin at two because the action applies to closing multiple tabs. [Native new-tab and close rules](../../src/tab_options.rs)
- Eager loading for pinned tabs affects the next session restore and applies while other restored tabs wait to be selected. Workspace scrolling, counts, and rules require workspaces to be enabled. Tab groups use the site-island presentation with one automatic-grouping preference. Native settings ignore the retired style, joining, drag, delay, editor, and compact-row options on older profiles. [Native tab options](../../src/tab_options.rs), [workspace rules](../../src/app/stacks.rs)
- The custom background, text, and highlight are independent overrides. An empty value lets Bowser choose the corresponding theme colour. [Native theme options](../../src/theme_custom.rs)

### Preserve search engine compatibility and disclose network behavior

Keep `optimised` as the persisted ID for Bowser's DuckDuckGo HTML default. The request template disables sponsored results with `k1=-1`. The main DuckDuckGo site remains a separate engine, shown as “DuckDuckGo.com” to distinguish it from the HTML default while keeping stored IDs compatible. [Built-in search engines](../../src/search_engines.rs), [menu labels](../../ui/src/address-settings-logic.ts)

Give Search suggestions a visible explanation below its control because this preference can send typing to the engine before Enter. Native suggestions skip private tabs. [Settings row copy](../../ui/src/address-settings-logic.ts), [native suggestions](../../src/address_suggest.rs)

Refresh subscribed feeds when the panel opens and the configured interval has elapsed, or when the user presses Refresh. The `0` wire value means manual refresh only. Keep the UI choices aligned with native `REFRESH_CHOICES` so every offer is accepted by the native command. [Feeds panel](../../src/win32/feeds_panel.rs), [feed refresh choices](../../src/feeds/mod.rs), [parity test](../../ui/test/bars.mjs)

## Consequences

Update the native definition, TypeScript control, and relevant contract test together when changing an option. When a preference's runtime prerequisites change, update its Settings applicability rule too.
