# Menu icon map (defects D2, D3)

Rule: every row of a page that has any icon either shows its own distinct glyph from the icon family
(`ui/src/icons.ts`) or keeps the 16px icon column empty so labels line up. One glyph is never used for
unrelated actions inside a menu. Check marks are a trailing column (never an icon); submenus show a
trailing chevron. The HTML menu draws only `Item::glyph("...")`; the legacy native `Icon` enum is
ignored except for unambiguous glyphs (Grid and Check are never sent). New icons: `link`, `droplet`.

Geometry: 28px rows (the 32px component height would scroll the 15-row main menu on 720px-tall
work areas), 4px radius, 12px icon gap, 8px overlay radius, min 200 / max 360px, opaque overlay.

## Main menu (`main`)
The menu is short on purpose: what you do on every page, and nothing you set once. Profile card (header) |
New tab `plus` | New private tab `private` | Close all private tabs `glasses` (only while a private tab is
open) | History `history` (opens `bowser://history`) | Bookmark this page `bookmark` | Workspaces
`workspace` (submenu) | Block ads & trackers `shield` (check) | Reader view `reader` (check; a capital A beside text lines over a full-width line, so it
stays apart from the page-shaped `notes` and the book `workspace-book`) | More tools
`more` (submenu: Notes & Tasks `notes`, Command palette `command`, Saved sessions `folder`, Hide
distractions `eye`, Extensions `puzzle`, Passwords & passkeys `key`) | Restart to update `refresh` (only
once a build is downloaded) | Settings `gear` (opens `bowser://settings` in a tab; the gear turns one
tooth while the row is selected, docs/design/settings-page.md) | Exit `close`. Moved to Settings: the
profile list (Manage profiles), Import from another browser, the Privacy dashboard, Check for updates and
Update automatically. Passwords stay in the native menu: `chrome_host/guard.rs` keeps that word out of
the HTML chrome.

## Profile (`profiles`, also the main menu's Profile page)
Profile rows: avatar badge (disc with the profile's accent), check on the current one | Guest window
`external` | New profile `plus` | Choose avatar `person` (submenu, check on current) | Choose accent
colour `droplet` (submenu, colour swatch badges) | Delete profile section rows `trash` | Rename
profile section rows `edit`. Confirmations: Delete profile `trash`, Keep profile `person`; notice:
Close `close`.

## Workspaces (`workspaces`, main menu Workspaces page)
Each workspace: its own glyph `workspace-briefcase|home|code|book|cart|game|music|heart|star|bolt|globe|leaf`
(check on active) | New workspace `plus` | Rename current `edit` | Delete current `trash`.
Delete confirmation: Delete `trash`, Keep `workspace`.

## Rail workspace (`rail-ws:*`)
Rename `edit` | Change icon `workspace` (submenu of the twelve workspace glyphs) | Change colour
`droplet` (submenu of colour swatches) | Move up `chevron-up` | Move down `chevron-down` | Delete `trash`.
Rail web app (`rail-app:*`): Open `app-tile` | Edit `edit` | Remove `trash`.

## Tab context (`tab:*`)
New tab to the right `plus` | New private tab `private` | New split view `split` | Add tab to group `tab-group` (submenu, check on
the standing mode, no icons) | Collapse/Expand group `chevron-up`/`chevron-down` | Remove from group
`caption-minimize` (a dash) | Undo tab organisation `reset` | Copy URL `link` | Reload `reload` |
Duplicate tab `copy` | Pin tab `pin` (check) | Mute tab `mute` / Unmute tab `volume` | Mute/Unmute site
`workspace-globe` | Hibernate tab `clock` | Move tab to new window `external` | Picture in picture `pip` |
Close tab `close` | Close other tabs `trash` | Close tabs to the left `back` | Close tabs to the right
`forward` | Close all private tabs `glasses` (private tabs only; docs/design/private-tabs.md).

## Split (`split`)
Active split: Swap panes `reload` | Close left pane `back` | Close right pane `forward` | Replace pane
with tab `split` (submenu). Not split: Open split view with a new tab `plus` | Open split view with
`split` (submenu). Tab rows: favicon, else `workspace-globe`.

## Open tabs overflow (`open-tabs`)
Every row: the tab's favicon (PNG data URI, gated like the tab strip's, deduplicated in a `favs` list);
tabs without one show `workspace-globe`. Active tab: trailing check.

## Site information (`site-info`)
Connection `lock` / `unlock` / `warning` / `info` | Certificate details (no icon, column reserved) |
Microphone `mic` | Camera `camera` | Location `location` | Notifications `bell` (each a submenu with a
check on the stored choice) | Cookies `cookie` | Clear cookies `trash` | Blocker `shield` (check) |
Tracking prevention `eye` | Reset permissions `reset`.

## Extensions (`extensions`)
Each extension `puzzle` (check = enabled) | Remove extension `trash` (submenu rows `puzzle`) | Install
unpacked extension `folder`.

## Saved sessions (`sessions`)
Save current tabs `plus` | each session `folder` (submenu: Restore N tabs `reset`, Delete session `trash`).

## Window system menu (`system`)
Restore `caption-restore` | Minimize `caption-minimize` | Maximize `caption-maximize` | Close `close`
(Move and Size have no icon; the column is reserved).

## Other
Import offer: Import `download`, Not now `clock`, Don't ask again `close`. Bookmark toast: `bookmark`.

## Disabled rows
`--bw-text-2` at 65% over the solid overlay: at least 3:1 in dark and light (checked in
`ui/test/menu.mjs`), clearly dimmer than full-strength rows.
