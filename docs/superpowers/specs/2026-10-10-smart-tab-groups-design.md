# Smart Tab Groups Design

## Goal

Group related Bowser tabs by registrable domain in automatic mode while preserving the existing ordered tab model, individual frosted-glass tab islands, active-tab identity, and lazy WebView creation.

## Behavior

- HTTP and HTTPS tabs receive a canonical grouping key from their committed URL's registrable domain, using the Public Suffix List including private suffixes. Scheme and port do not affect the key. Non-web URLs, IP addresses, and hosts without a registrable domain do not auto-group.
- Automatic groups form when at least two unpinned tabs share a key. The tab model order is never changed by automatic grouping. In expanded mode every tab remains its own island; group members receive a subtle shared visual cue.
- A group can be collapsed from its group affordance. The affordance represents the group at its first member in tab order; collapsing hides the other members while keeping the active member visible. Expanding restores members in their original order. Selecting a hidden member expands its group before activating it.
- A domain context action can set Auto, Always, or Never. Auto requires two members; Always can show a singleton group affordance; Never leaves matching tabs ungrouped. These overrides apply only to unpinned tabs.
- Pin and Unpin are explicit tab actions. Pinned tabs occupy a stable leading partition, are excluded from groups, and keep relative order within their partition. Pinning/unpinning moves only the acted-on tab and preserves the active TabId.
- A tab's grouping key changes only when its committed address changes. Pending/optimistic address-bar text, title, favicon, and loading events do not affect grouping.

## Persistence

- Persist only versioned per-domain grouping override and collapsed-state preferences in Bowser's existing local settings area. Do not persist tab identities, URLs, credentials, or session contents.
- Load preferences once at startup and save on user changes with a bounded, atomic write. No polling or background worker is added.
- A collapsed preference applies when a matching group next exists, including after restart if the user opens matching tabs. It never changes tab order or active selection.

## Architecture and UI

- Keep the existing `TabList` / `AppTab` records as the source of truth; add pin metadata and derive visible group slots from current tabs and preferences.
- Group calculation and layout are pure metadata operations. They must not create/select WebViews, alter hidden-tab timers, or add recurring work.
- Keep existing individual frosted tab islands when expanded. Add a compact frosted group affordance, member count, collapse control, and minimal shared accent. Reuse the existing tab strip and context-menu visual language.
- Hit testing maps visible slots back to existing tab identities. Closing, selecting, keyboard navigation, reorder, and viewport scrolling continue to address model tabs consistently.

## Security and performance

- Never derive groups from website messages or page-controlled metadata; use Bowser's committed navigation state.
- No website receives group or pin controls. No WebView2 settings or browser security policies change.
- Use a maintained PSL implementation with local data; no network lookup, background refresh, or per-tab WebView work.

## Tests and acceptance

- Test registrable-domain extraction (including private suffixes, IDNs, trailing dots, ports, IPs, and non-web schemes), Auto/Always/Never thresholds, pinned partition and exclusion, non-contiguous membership without reordering, collapsed active-member visibility, selection expansion, stable active identity, and persistence round trips/corrupt-file fallback.
- Run focused Rust tests and existing formatting, Clippy, test, and build checks. Manually verify the tab strip with collapsed/expanded groups and pinning if the Windows UI runner is available.
- No commit is made.

## Addendum (2026-11): groups made by hand, and new tabs in a group

Until this addendum every group was a *site* group: `TabGroupKey::Domain`, formed automatically from the
registrable domain (or forced by "Always", "Group tabs of this site", "Group all tabs by site" or a drag, which
all still keyed the group by a site). There was no way to put tabs of different sites in one group.

### Data model

- `TabGroupKey` has a second variant, `Manual(ManualGroupId)`. A manual group has a name (default "Group N",
  N being its id), a colour and a collapsed flag (`tab_groups::ManualGroup`), kept beside the site preferences
  in `GroupPreferences` so every projection (strip, sidebar, bookmarks bar, wire rows) sees them through one
  value. `group_name`, `group_colour`, `group_collapsed` and their setters answer for either kind of key.
- A tab's place in a manual group is `tab_organisation::Membership::Manual(id)`. `apply_membership` gives such a
  tab its group and clears its site, so the projection (`tab_groups::all_groups`) puts it in that group only: a
  manual group forms with a single tab and any mix of sites; site groups are then formed from the remaining tabs.
  Pinned and private tabs are never in any group.
- A manual group lives as long as a tab is placed in it: leaving, moving, ungrouping or closing its last tab
  forgets it (`App::prune_manual_groups`). Ids are one more than the highest in use, so they stay small.
- Wire keys hash `m:<id>` (site groups keep `d:<domain>`), so the HTML strip and sidebar need no change: they
  already draw any group from its key, name, colour and fold.

### Persistence

Manual groups are not written to `tab-groups.txt` (which stays per-site preferences only). They belong to their
tabs, so the session snapshot (`session.txt`) carries them: a `group\t<id>` line after a tab's line, and one
`tabgroup\t<id>\t<colour|->\t<0|1 collapsed>\t[name]` line per group that a saved tab is in. Older builds skip
both kinds of line. A restore makes the groups again under fresh ids of the window and puts the restored tabs
back in them; a group line no tab names, or a tab naming an unknown group, is dropped.

### How automatic grouping and manual groups meet

Automatic grouping never takes a tab out of a manual group. Concretely: the site projection ignores manual
members; "Group this website's tabs", "Group all tabs by website" and "New tab position: grouped with related"
skip them (`stacks::StackInput::manual`); an "Always" site rule does not reach them. Only the person moves a tab
out: "Remove from group", "Ungroup", "Add tab to new group" or "Move tab to another group", dragging it out of
the group (or between two members of another group), or closing it.

### Tab menu

The "Tab group" submenu now starts with *Add tab to new group* (opens the name and colour editor at once),
*Add tab to group ›* (every other group of this workspace, manual or site; *Move tab to another group ›* for a
grouped tab) and *Remove from group*. Then the group's own actions, and last a "Group by website" heading with a
one-line explanation under it ("Tabs from the same website, such as all your GitHub tabs, share one group"),
*Group this website's tabs*, *Group all tabs by website* and the site's Always / automatically / Never rule.
The bookmarks bar keeps its group chips per site, so a manual group has no chip there.

### New tabs in a group

Ctrl+T or the new tab button while the shown tab is in a group, and *New tab to the right* on a grouped tab,
open the new tab in that group (`App::join_group_of`): a manual group's membership, or the site group's
`Joined` placement, which keeps the tab there whatever site it then opens. When "New tab position" would put it
elsewhere (as last), it moves right after the group's last tab. Settings > Tabs > New tabs > "Keep in group"
(`tabs_new_tab_in_group`, on by default) turns this off. A private tab never joins a group.
