# Settings structure (`bowser://settings`)

Owner: settings. Status: implemented. Companion to `settings-page.md` (how the page is served, the
bridge, the row kinds and motion). This note is the information architecture: which nav sections
exist, which groups each holds, which rows are shown and which sit behind the section's **More**, and
what **Simple** shows. The table that drives it is `AREAS` (and `SIMPLE_GROUPS`) in
`ui/src/settings-page-logic.ts`; `ui/test/settings-page.mjs` checks that every row is placed exactly
once.

## Problems with the old arrangement (audit, November 2026)

- 230 rows in 9 nav sections, but the placement came from three unrelated places: the Rust section a
  row was born in, the card it was built into, and a core/More tier. Related rows ended up apart:
  page zoom in Appearance, zoom scope and wheel zoom in Browsing > Page tools; the address bar's look
  in Appearance, its behaviour in Browsing; translation in Appearance; Ctrl+Tab order in Tabs, the
  other chord switches in Shortcuts; Performance inside Privacy; Default browser inside About; the
  start page's dials inside General with the startup rows.
- More hid things people do change (cookie notices, YouTube ads, history retention, sleep timer,
  new tab position, page zoom, the address bar's centring and short address), while rarely changed
  page tools (link keys, capture file name, zoom scope) were in view.
- Appearance's shown part was a column of single-row cards whose headings were hidden (Toolbar,
  Bookmarks bar, Text size, Offer translate): a card grid without labels.
- There was no way to put the tabs down the side from the Tabs section; it lived only in the
  Appearance layout presets.
- Simple was 11 rows, three of them separate implementations (a Light / Dark / System switch, a
  Top / Side switch, an Extensions button) and the search engine shown as text that could not be
  changed. It had no blocking, zoom, text size, toolbar, downloads or updates.
- Headings were cramped: the section title had no space under it, so "About" sat on "Updates", and
  sub-headings had the same colour as the title.

Old placement (shown rows, then More; **[S]** = in Simple):

| Section | Part | Shown | More |
|---|---|---|---|
| General | Profile | Profile, Profiles, Import **[S]** | |
| | Search | Search engine **[S]**, Engines | 15 search, search field and nickname rows |
| | Start page | Homepage **[S]**, Startup **[S]**, Startup pages | 12 dial and widget rows |
| | Downloads | Folder, Ask where | |
| Appearance | (one part) | Presets, Palette, Accent **[S]**, Websites, Mode, Toolbar, Bookmarks bar, Text size, Offer translate | 46 rows: layout, theme, address bar look, window, panels, bookmarks bar, font, page zoom, translation |
| Tabs | Tabs, Groups | New tab page, Page address, Workspaces, Rules, Auto group | 38 tab rows incl. Ctrl+Tab and previews |
| Privacy | Privacy, Performance | Blocking, GTA VI spoilers, Tracking, History, Site rules, Block lists, Clear data **[S]**, Dashboard, Efficiency, Sleep tabs | Cookie notices, YouTube ads, 5 spoiler rows, Skipped sites, Keep history, Sleep after |
| Browsing | Address, Bookmarks, Page tools | Suggestions, HTTPS, Feeds, New tab, Folder, File name, Link keys, Zoom scope, Wheel zoom | 19 address rows, 4 bookmark rows, 10 page tool rows |
| Shortcuts | Keyboard, Commands | Shortcuts, Bindings, Chains | Ctrl+S, Shift+right-click, Ctrl+Shift+C, 4 keyboard rows, 6 Quick commands rows |
| Extensions | | Extensions | |
| Media | | Playback | |
| About | Updates, About | Automatic, Status, Version, Settings file, Reset, Default browser **[S]** | |

## New structure

Rules:

1. A nav section holds **groups**; a group is one sub-heading over one card. The same group title
   is used in the shown part and inside More, so a row never changes name when it is folded away.
2. A row lives in exactly one group, listed in `AREAS`. A row nobody listed (a new one) joins the
   first group of its Rust section, shown or in More by its tier, so adding a row is one line.
3. Shown rows are what people change; More is for tuning. No group is shown as a bare card.
4. Rust's section keys and deep links are unchanged. A deep link scrolls to the first group of that
   section (opening More when the group lives only there).
5. Setting semantics and storage keys are unchanged. Two Simple controls became real rows of All
   settings (Mode, Tab position); nothing else is new.

Shown rows first, then More (in brackets):

| Nav | Group (deep-link section) | Rows |
|---|---|---|
| **General** | Profiles (`profile`) | Profile, Profiles, Import |
| | On startup | Startup, Startup pages, Homepage |
| | Search engine (`search`) | Search engine, Engines (sub-page) [Private search, Image search, Address search, Address suggestions, New tab, Selection search] |
| | Search field | [Search field, Nicknames, Engine icon, Remember engine, Keep text, Suggestions] |
| | Search scopes | [Bookmarks, History and Tabs nicknames] |
| | Downloads (`downloads`) | Download folder, Ask where |
| | Default browser | Default browser |
| **Appearance** | Theme (`appearance`) | Mode (Light / Dark / System), Accent, Palette, Websites [Schedule, Look, Schedule times, Colours, Bar tint, Page accent, Saturation, Contrast, Theme file, Force dark, Private theme] |
| | Layout | Presets [Top style, Sidebar width, Pin sidebar, Tab bar, Address bar, Title bar, Status bar, Density, Compact menus] |
| | Toolbar | Customise your toolbar, Centre address, Short address, Bookmarks bar [Load progress, Full address, Address field, QR code] |
| | Text and zoom | Text size, Page zoom [Font, UI zoom, Zoom indicator, Zoom scope, Wheel zoom] |
| | Bookmarks bar | [Groups on bar, Bar folder, Bar position, Bar display, Pin groups] |
| | Window | [Rounded page, Auto-hide, Hidden bars, Full screen, Animation, Steppers, Fade inactive, Scrollbars, Plain icons, Reset] |
| | Panels | [Close button, Folder counts, Auto-close, Reset] |
| **Tabs** | Tab bar (`tabs`) | Tab position (Top / Side) [Keep tabs wide, Active tab min width, Wheel switches tabs, No gap when maximised, Load progress, Click active to go back, Unread dots, Title counts as unread, Dim sleeping, Fade unused tabs after, Preview images, Preview memory] |
| | New tabs | New tab page, Page address, New tab position [Links in background, Duplicated tab, Keep in group, Focus page] |
| | Start page (`startpage`) | Speed dials, Tile size, Site names, Max columns [Add site tile, Group to show, Drag to reorder] |
| | Start page widgets | Widgets [Widget columns, Widget background, Custom colour, Widget height, Drag to rearrange] |
| | Groups and workspaces (`stacks`) | Auto group, Workspaces, Rules (sub-page) [Wheel switches workspaces, Workspace tab counts, Edge split, Tile titles, Tight tiles] |
| | Closing | [Back to opener, After close, Close under pointer, Keep window, Confirm close, Double-click, Close on left, Close on every tab] |
| | Pinned and restored | [Close pinned, Pinned separator, Stay on site, Load tabs when selected, Load pinned at once] |
| | Selecting and audio | [Multi-select, Include active tab, Tab audio] |
| **Privacy** (Privacy and security) | Blocking (`privacy`) | Blocking, Cookie notices, YouTube ads, Block lists (sub-page) [Skipped sites] |
| | History and tracking | History, Keep history, Tracking |
| | Site data | Clear data, Site rules, Dashboard (sub-pages) |
| | Security | HTTPS [Base domain, Strip scripts] |
| | Spoiler protection | GTA VI spoilers [Hide, Simple covers, Scan images, Term updates, Spoiler limits] |
| **Browsing** | Address bar (`address`) | Suggestions [Select all, Encoded copy, Expansion] |
| | Address bar suggestions | [Typeahead, Badges, Typed history, Typed searches, History button, Result limit, Bookmarks first, Direct match, Sources] |
| | Bookmarks (`bookmarks`) | New tab [Single click, Confirm many, Page details, Edit dialog] |
| | Translation (`pagetools`) | Offer translate [Auto-translate, Translate to] |
| | Screenshots | Folder [File name, Format] |
| | Reader view | [Font, Text size, Line height, Column width, Colours, Vertical text, On-page controls, Reading time] |
| | Page tools | [Picture in picture, Link keys] |
| | Feeds | Feeds [Feed button, Refresh] |
| **Shortcuts** | Keyboard (`keyboard`) | Shortcuts, Bindings (sub-page) [Single-key, Alt menu, F6 focus, Site shortcuts] |
| | Keys and clicks | Ctrl+Tab, Ctrl+S, Ctrl+Shift+C, Shift+right-click |
| | Quick commands (`commands`) | Chains (sub-page) [Result types, Result limit, Remember search, Open in tab, Notes panel, Nicknames] |
| **Extensions** | Extensions (`extensions`) | the extensions block |
| **System** | Performance (`performance`) | Efficiency, Sleep tabs, Sleep after |
| | Media (`media`) | Playback (the on-device check) |
| **About** | Updates (`updates`) | Automatic updates, Status |
| | About Bowser (`about`) | Version, Settings file, Reset |

A Tabs row that cannot act rests: disabled and out of the page (`is-na`, a search still finds it)
until it can (`tabRowRests` in `ui/src/tab-options-logic.ts`). The tab bar's own options (Keep tabs
wide, Active tab min width, Wheel switches tabs, No gap when maximised, Close under pointer) while
tabs are on the side; Focus page while a new tab opens no page; Title counts as unread without Unread
dots; Include active tab without Multi-select; Load pinned at once without Load tabs when selected;
Wheel switches workspaces, Workspace tab counts and Rules without Workspaces.

What moved, in short: Performance left Privacy for System (with Media, which no longer has a nav
entry of its own); Default browser left About for General; the start page's dials and widgets left
General for Tabs > Start page (where the start page's own "Settings" link lands), the startup rows
stayed in General; translation left Appearance for Browsing; zoom scope and wheel zoom joined page
zoom in Appearance > Text and zoom; HTTPS, base domain and script stripping left the address bar for
Privacy > Security; the address bar's display options (full address, address field, QR code) joined
its centring and short address in Appearance > Toolbar; Ctrl+Tab order joined the other chord
switches in Shortcuts > Keys and clicks; the bookmarks bar's own options got one group in
Appearance. Promoted out of More: Cookie notices, YouTube ads, Keep history, Sleep after, New tab
position, Page zoom, Centre address, Short address, the four chord switches and three start page
rows. Moved into More: Zoom scope, Wheel zoom, Link keys, capture file name, and the theme schedule
(the new Mode row covers light, dark and system).

## Simple

Simple is the same rows, lifted out of All settings while Simple is on and put back when it is off
(the nodes move; nothing is built twice), in four groups with the same sub-heading style:

| Group | Rows |
|---|---|
| Startup and search | Startup, Startup pages (only while "Specific pages" is chosen), Homepage, Search engine |
| Appearance | Mode, Accent, Tab position, Customise your toolbar, Text size, Page zoom |
| Privacy | Blocking, Clear data |
| Your browser | Profile (with Manage, which opens All settings at Profiles), Import, Download folder, Default browser, Automatic updates, Status |

Passwords are not on the page: the chrome is not allowed to name the password store
(`chrome_host/guard.rs`), so they stay in the main menu. Extensions are one click away through All
settings.

Search works the same in both views: typing in Simple shows matching rows from all of All
settings, and clearing the search returns to Simple.

## Spacing

Section title (Subtitle 20/28) then 16px (`--bw-space-4`) to the first group; group heading (Body
strong 14/20, secondary colour) then 8px to its card; 24px (`--bw-space-6`) between groups; 56px
between sections. Every gap is a margin on the element below it, so a hidden group or heading
leaves no stray space.
