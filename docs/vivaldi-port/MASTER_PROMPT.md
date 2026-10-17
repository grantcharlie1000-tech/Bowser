MASTER PROMPT: Vivaldi feature port for Bowser
You are the orchestrator, running in Claude Code.

CONTEXT
Bowser is a native Windows browser (Rust + WebView2; HTML chrome in ui/, Rust in src/). We are adding the Vivaldi features listed below. This prompt is the full spec. Background text scraped from vivaldi.com is in C:\Users\charlie\.claude\vivaldi-port\vivaldi-features.txt (read the section for your feature). Before anything else read PRODUCT.md, ui/STYLEGUIDE.md, ui/README.md, docs/html-chrome-plan.md, docs/html-chrome-protocol.md and docs/sidebar-panel-host.md.

This prompt is saved at C:\Users\charlie\.claude\vivaldi-port\MASTER_PROMPT.md.

RE-READ RULE
After every step (step 0, step 1, each thread finishing, each wave finishing) re-read this whole file and compare what was done against it. Fix any drift before moving on. Every subagent brief must tell the subagent to do the same after each of its own steps.

RULES
- UI style does not change. Reuse existing tokens, components, icons (existing icon pipeline, no new icon families) and motion. New surfaces must look made by the same hand as the existing ones. Layout presets use preview thumbnails in the style Bowser's layout settings already have; add new thumbnails the same way.
- A setting must do something. No row is added unless it changes behaviour, and each new row needs a test that proves the effect (UI -> Rust save -> snapshot -> behaviour). If a Vivaldi option has no meaning on Windows/WebView2, say so and skip it rather than adding a dead toggle.
- Local main is far behind origin/main (39 commits when checked). All work branches from origin/main. Never touch local main or the uncommitted file benchmarks/results/sidebar-verify/run2-B/http-requests.jsonl.
- Subagents: use the Agent tool with isolation:"worktree", run_in_background, model "sonnet" / "opus" / "fable" as listed. The worktree starts from the local HEAD, so each subagent's first command is: git fetch origin && git checkout -B <branch> origin/main. At most 5 subagents at once.
- Shared files: settings chain (settings.rs, chrome_host/settings.rs, ui/src/settings-page-logic.ts, settings-page.ts, ui/test/settings-page.mjs, win32/settings_panel.rs), chrome_host/protocol.rs and ui/src/protocol.ts, win32/window.rs, app.rs, chrome/layout.rs, theme.rs, ui/src/view.ts, shortcuts.rs. Merge settings-touching PRs one at a time; each thread rebases on origin/main and rebuilds ui/dist and ui/dist.manifest before its PR (rebuild, don't hand-merge).
- Test data: redirect LOCALAPPDATA for every test launch; benchmark flags don't isolate app data.
- Never drive GUI tests on the user's desktop. Headless only: cd ui; npm run build / lint / typecheck / test; cargo test --locked; cargo clippy --locked -- -D warnings; headless screenshot harnesses (benchmarks/results/foundation/harness/render.mjs, benchmarks/results/surfaces/panels2/harness/render.mjs). Real-app checks are one batched pass the user approves.
- Every UI change gets headless before/after PNGs (light and dark, 100% and 150%).
- Release builds use --features html-chrome-default. Merge means release and the version must equal the release tag, so never merge. Each thread may commit to its own branch and open one PR with the file-pr skill; the user merges. Use monitor-pr only if asked.
- PRODUCT.md applies: nothing may delay input, focus or navigation or slow cold launch. Passwords stay native.
- Skills: verification-before-completion before any "done" claim; security-audit for threads marked (security); using-git-worktrees if the Agent tool's worktree isolation is unavailable; code-review at the end.
- Never delete a setting or feature without asking the user.

STEP 0: SETUP
1. Save this prompt to the path above if it is not already there. git fetch. Confirm the baseline is origin/main.
2. Report the ahead/behind count and which features origin/main already has.

STEP 1: SETTINGS AUDIT (read-only, before any feature work)
Audit every row of the settings page (ui/src/settings-page-logic.ts ROWS and SECTIONS, src/settings.rs, src/chrome_host/settings.rs, win32/settings_panel.rs fallback, plus the sections origin/main adds). For each row record: key, label, the code that acts on it, test coverage, verdict: WORKS / NOTHING (saved but never acted on, or marked notYet) / PARTIAL / MISMATCH (label differs from behaviour). Known suspects: frameless, the bookmarks bar. Verify by reading code and headless tests, not by launching the app. Present the table to the user. For each NOTHING/PARTIAL/MISMATCH row: if a thread below owns that feature, assign it to that thread; otherwise ask the user whether to implement or remove. Wait for the user's go-ahead before wave 1. Repeat this audit at the very end across everything that was added.

WAVES
Wave 1 (5 parallel): T1 to T5.
Wave 2: T6 Layout and T7 Page tools.
Wave 3 (after Layout merges): T8 Themes, T9 Window and Panels, T10 Stacking/tiling/workspaces, T11 Auto-hide.
Wave 4 (after Commands and Layout): T12 Mouse gestures, T13 Toolbars and feeds.
Final: a fable subagent runs code-review and security-audit on the combined result, and the step 1 audit is repeated.

THREADS (every row listed must work and be tested)

T1 TABS (sonnet)
- Undo close tab: Ctrl+Shift+T reopens the last closed tab; a Closed Tabs button/menu lists recently closed tabs this session; clicking reopens.
- Periodic reload: per-tab, from the tab right-click menu; interval presets; countdown shown on the tab; disable one / disable all.
- Sessions: save all open tabs or only selected tabs as a named session; open, delete; a Sessions panel (Save Tabs as Session button, double-click to open); Quick Commands "Save All Tabs as Session" and "Open Saved Session".
- Tabs settings: show tab bar; new tab page (start page / homepage / blank / specific URL); new tab position (after related / after active / as last / as stack with related); new tab from link opens in background; duplicated tab position (next to original / last); close tab activation (always activate related tab; recently used / left / right); align next close button; keep window open when last tab closed; confirm when closing N or more tabs; focus page content on new tab; double-click action (rename / close / ignore); horizontal scrolling; remove tab spacing when maximised; popup thumbnails; tab memory usage in popup; tab thumbnails; close button (left side, permanent); detect page title notifications; unread indicators; dim icon when hibernated; active tab minimum width; inactive tabs threshold in days; page load progress (bar / favicon progress / favicon spinner / none); tab cycling (recently used / tab order); tab cycler and list; switch tabs by scrolling; minimise active tab; tab selection (enable, include active tab); mute tab audio (play all / only active / prioritise active); pinned tabs (don't close / minimise instead of close / close as other tabs; separator below pinned; restrict pinned tabs to current site); lazy-load restored tabs and always load pinned.

T2 ADDRESS BAR, SEARCH, BOOKMARKS (sonnet)
- Address bar: show/hide; open in new tab with Alt+Enter or Shift+Enter; autocomplete; show full address; select address on activation; copy and cut encoded address; page load progress bar; QR code generator; drop-down: show while typing, always show item badge, typed history drop-down (include search queries, history button), maximum results; suggestion sources and order: bookmarks (prioritise), bookmark nicknames, search suggestions (only with engine nickname), search history, history, direct match (prioritise), local calculator; security: always use HTTPS, strip JavaScript from pasted text, highlight base domain; domain expansion with Ctrl+Enter and a configurable suffix.
- Search: default engine, private-window engine, image search engine; search field display as text field or button; show nickname in drop-down; always search in new tab; show engine icon; search in address field; keep last selected engine; keep typed text after searching; page selection search in background; suggestions in the address field (only with nickname) and in the search field; engine editor (name, nickname, URL with %s, POST parameters, suggest URL, image search URL and POST parameters, set as default, set as private, add/remove/reorder, restore defaults); scoped nicknames @bookmarks, @history, @tabs (each can be turned off and renamed).
- Bookmarks: open in new tab; open panel items with single click; confirm when opening N or more; fetch bookmark data from page metadata; always open bookmark dialog; bookmark bar (show, folder, top/bottom, display as text+icon / text / icon / bookmark icon only).

T3 PRIVACY, HISTORY, WEBSITE APPEARANCE (sonnet, security)
- Automatic cookie-warning removal (EasyList Cookie List and "I don't care about cookies" style rules), working without full ad blocking, with a per-site off switch.
- Blocking level none / trackers / trackers and ads, global and per site, with an exceptions list; manage sources (enable/disable, add by URL or file); shield shows counts of blocked items.
- History: retention (session only, 1 day, 1 week, 1 month, 3 months default, 6 months, 1 year, forever); Delete Browsing Data dialog (past hour / day / week / 4 weeks / everything; history, cookies, storage, cache, passwords, form autofill, site settings).
- Private window: its own default search engine and its own theme.
- Website appearance: Light / Dark / Auto, plus force dark theme on all websites.

T4 COMMANDS (sonnet)
- Quick Commands: one command registry that every command, shortcut, chain and gesture uses. The palette searches open tabs, closed tabs, workspaces, bookmarks, notes, history, commands, page actions and does calculations. Filter prefixes bookmark: note: tab: history: command: action: workspace: with short forms bkm: tb: cmd:. Shift or Right arrow autocompletes. Settings: which result types show and their order (with reset), keep last typed value, limit visible results, open links in new tab, open bookmark automatically on nickname match, open notes in Notes manager, include English search terms.
- Keyboard: every command assignable, several shortcuts per command, clear, restore one / restore all, cheat sheet (Ctrl+F1), enable/disable shortcuts, single-key shortcuts on/off, Alt opens the menu on/off, browser-priority shortcut list and a per-site "prioritise website shortcuts", focus cycling options, access keys on/off, Space fast-forward on/off.
- Command chains: create, clone, edit, delete; ordered commands with a delay in ms; Test Chain; run from Quick Commands, a keyboard shortcut, a gesture or a toolbar button.

T5 START PAGE (sonnet)
- New tab page shows speed dials: bookmark folders flagged as speed dial, groups, add/edit/delete, drag to reorder, custom thumbnail, sub-folders. Settings: reopen with last group / first group; titles always / never / when needed; show add button; maximum columns; five thumbnail sizes; drag reorder on/off; homepage; startup with start page / last session / homepage / specific pages.
- Dashboard widgets: top sites, bookmarks (choose folder, show titles and favicons or thumbnails), sticky notes (save to a note), date, privacy statistics, a webpage widget. Settings: enable widgets, maximum columns 1 to 4, regular or tall height, background style solid / semi-transparent / transparent / custom, drag to rearrange.
- The start page must not slow new tab open.

T6 LAYOUT (fable)
- Layout presets: Simple, Classic, Vertical Left, Vertical Right, Auto-hide, Bottom, plus Saved (the user's own configuration, kept up to date), each with a preview thumbnail in the existing style.
- Tab bar position top / left / right / bottom and show/hide. Address bar top / bottom and show/hide. Panel left / right. Status bar: show / info overlay only / hide. Show title bar when the tab bar is on a side. UI density regular / compact and compact menu layout. UI zoom slider with reset and typed value.
- Native geometry in chrome/layout.rs, the HTML host bounds and the GDI fallback must agree; --bw-toolbar-h must equal STRIP_HEIGHT.

T7 PAGE TOOLS (sonnet, security)
- Capture: visible area, full page (up to 30000 px) and selection; save as PNG or JPEG, copy to clipboard, create a note; capture storage folder and file name template settings; reachable from the palette and a shortcut.
- Spatial navigation: Shift+arrow keys move between links; Enter opens, Ctrl+Enter opens in a new tab; on/off setting.
- Notes: add selected text to a new note (Ctrl+Shift+C), append to an existing note, insert a note from a text field's context menu, note with capture, markdown, folders, sort (manual / title / address / content / date), export and import as .md/.txt.
- Reader view settings: font face and size, line height, column width, colour scheme, vertical text, settings icon on the page, reading-time counter.
- Page zoom: default webpage zoom, tab zoom vs site zoom, Ctrl+scroll to zoom. Pop-out video: on/off for the video button. Translate panel: use panel instead of dialog; translate when pasting text.

T8 THEMES (sonnet)
- Colours: background, foreground, highlight, accent; prefer system colour; colouring mode (accent on tab bar / on address bar / unified); get accent colour from the webpage; limit accent saturation; colour contrast. User colours must still pass the contrast tests in theme.rs.
- Theme schedule: none / follow operating system (light theme, dark theme) / manual time schedule.
- Import and export a theme file; reset themes settings.

T9 WINDOW APPEARANCE AND PANELS (sonnet)
- Window appearance: open settings in a tab; open popups in tabs; use animation (honour prefers-reduced-motion); buttons instead of sliders in range controls; show title bar; fade colours in background windows; simple scrollbars; monochrome settings icons; reset toolbars.
- Panels: show panel toggle; floating panel with auto-close; close button; folder counters. Web panels: detect page title notifications, lazy load, separate width per panel, zoom controls, periodic reload (1 to 30 minutes), mute, open page in tab/window/private window, removed panels restore. Windows panel: activate with single click, pinned folder, create stack by drag and drop, unread badge, show other windows, show/hide/reorder folders. Reading list panel: single click, unread counters. Reset buttons for each section.

T10 STACKING, TILING, WORKSPACES (opus)
- Tab stacks: styles compact / two-level (with compact option) / accordion (auto expand) / disabled; stack all tabs of one host or all tabs by host; name, colour, pin, hibernate, tile, save as session, bookmark all, copy all links, move to window/workspace. Options: open tabs in current stack, auto-open stack editor, allow stacking by drag and drop, stacking drop delay in ms. Update the native GDI fallback too.
- Tiling: vertical, horizontal, grid (Ctrl+F9 / F8 / F7), custom splits by dragging, resize, reorder, untile, open link as tiled tab; options: drag and drop tiling, title bar on each tile, remove space around tiles.
- Workspaces: enable, switch by scrolling, icon or emoji, tab counts, and rules that open matching sites in a named workspace automatically.

T11 AUTO-HIDE (sonnet)
- Enable UI auto-hide; choose which bars hide (tab bar, panel, address bar, status bar, bookmarks bar); always use auto-hide in fullscreen; toggle with Ctrl+F11, Quick Command and a status bar button; reveal by moving the pointer to the window edge. The frameless setting must either drive this or be removed (ask the user). Write the pointer-detection plan over the content WebView2 first and show it to the user before building.

T12 MOUSE GESTURES (sonnet, security)
- Allow gestures; right button or Alt key; mapping editor (new / edit gestures, command picker including chains); sensitivity slider; rocker gestures; sensible defaults. Show the user the input-capture approach before building.

T13 TOOLBARS AND FEEDS (sonnet)
- Toolbar editor: add, move and remove items on the tab bar, address bar, status bar and panel bar; reset each; command-chain buttons. Extension buttons: toggle only hidden / all, expand hidden into a drop-down.
- Feed reader: subscribe by URL, detect feeds on a page with an address bar icon and preview before subscribing, OPML import, read/unread, a Feeds panel, YouTube channels via no-cookie URL, podcast player. Fetch in Rust, reusing the existing WinHTTP code from translation.

EACH SUBAGENT BRIEF MUST INCLUDE
1. The thread's feature list above, copied in full, plus the RULES.
2. Files it owns and files it must not touch.
3. First step: a short plan listing existing code it will reuse and which of its settings rows already exist.
4. Definition of done: new rows each act and are tested, cargo test and clippy, ui lint/typecheck/test, headless screenshots, verification-before-completion output.
5. The instruction to re-read C:\Users\charlie\.claude\vivaldi-port\MASTER_PROMPT.md after each step.

WHEN EACH SUBAGENT REPORTS
Check it against its definition of done, re-read this file, summarise to the user in a few lines, then start the next eligible thread. Stop and ask the user before merging, releasing, adding network services or accounts, or dropping any planned feature.
