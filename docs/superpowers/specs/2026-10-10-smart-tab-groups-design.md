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
