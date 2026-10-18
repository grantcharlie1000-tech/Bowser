# Opt-in NewTab bookmarks-bar lifecycle pilot

Measurement only. No product memory target, suspension, prewarm, storage, parser budget, UI, security or focus-policy change. Earlier fast-input acceptance remains held. The prior sidebar slice removed duplicate controllers and improved a first-B UIA proxy, but its ten-pair result did not establish a RAM saving.

## Source-backed hypothesis

`src/win32/bookmark_bar.rs` creates a single HTML bar lazily, retains it when no longer wanted and hides with only `SetIsVisible(false)`/`SW_HIDE`. Its show path sends current theme/bookmark snapshot before visibility. Unlike the popup, it does not already request suspension/LOW targeting. Default `bookmarks_bar=never` does not create this host, so this is **not a default-startup RAM target**. A future bar-only LOW/NORMAL or confirmed suspend/resume experiment requires a preserved ordinary workload first and separate source review; no gain is assumed.

## Fixed baseline

Same exact immutable 19606 executable ON/OFF; one pair fresh/reused (four planned rows), normal `--data-dir`, ordinary updater-OFF setting, HTML default, no benchmark/dev/security-disabling flags. Retain all failures, exact pre/post-row binary hashes and fail-closed owned/profile-tree drain. No retyping, refocus, action-pattern fallback, replacement attempt or retry-to-pass.

Explicit synthetic settings: `bookmarks_bar=newtab`, `tabs_new_tab_page=blank`, existing default toolbar list plus its ordinary New tab button. The latter is not enabled by default. Store contains 818 synthetic bookmarks; native bar snapshots already cap the visible data at 200 items. First item ID1 has fixed label A and a unique loopback fixture URL. History remains 10,000 records. No real profile or arbitrary user data.

1. Launch a unique successful loopback fixture; require owned title/fixture ACK, native HTML toolbar/tabs/rail proof. One previously validated physical heading click activates the owned browser.
2. One guarded ordinary New tab button click scoped to the owned HTML toolbar document creates a blank tab, no start-page overlay. Require unique owned native `BowserBookmarkBar`, HTML Bookmarks bar and expected A button. Record first reveal proxy and first-shown resource phase.
3. One guarded physical A bookmark click must navigate successfully to its unique fixture; require exactly one ACK, exact owned title/visible document, hidden bar and the retained native container. Record first-hidden phase.
4. While hidden, atomically replace only the disposable synthetic bookmark store: ID1 changes label AND URL to B, other records unchanged. Record changed length/mtime/content; do not manipulate timestamps to force success.
5. One New tab click reveals the same native bar container. Require updated B label and absence of old A before any bookmark action. Record warm reveal proxy and re-shown resource phase.
6. One B bookmark click must navigate to B's distinct fixture and hide the retained bar again; record final hidden resource phase and a complete observation marker.

The tab trajectory is 1→2→3; phases are 2/2/3/3 tabs. Actual source-owned UIA TabItem and visible-document counts are recorded before/after each phase, not assumed. Phase differences therefore cannot be attributed exclusively to the bar. Cursor policy is no extra parking: it remains at each last predeclared clicked target identically in both arms; resulting hover work may occur and is not subtracted.

Each physical button click checks current enabled/visible source binding, point-to-target UIA ancestry, foreground ownership, neutral mouse/modifiers, physical single-monitor 96-DPI coordinates and immediate native point/root ownership via the existing guarded SendInput implementation. Partial injection blocks all later native lanes before diagnostic serialization. No permission/security/system focus workaround.

Resource phases use 1,000 ms settle plus three existing `measurements.psm1` native snapshots 250 ms apart, without supplied cached inventory. All owned/profile identities must remain stable before/after and occur in every native snapshot; gaps/churn are unavailable, not lower memory. Inventory/UIA/sampling costs and actual counts remain raw. Whole-tree private commit is not physical RAM/PSS, per-view ownership or a lifetime peak. No overhead subtraction or 20% claim.

ON additionally validates exact successful native navigation segments for A and B after log flush. OFF has no diagnostic stream; that evidence is unavailable, not passed. Both arms require identical external ACK/title/visible-page/hidden-bar gates. One native HWND proves retained container, not exclusive renderer/controller identity. First/warm reveal endpoints include input guards and UIA observation, not physical presentation or keyboard latency. No drag repair/instrumentation in this no-product baseline; existing product action/drag code is unchanged.

## Immutable input

Source `19606fc644a23c07db49faeae0b5d13497784649`, tree `cf214f415f80b408a0bbe7f16b990b3a34f7c55e`; EXE 7,938,560 bytes, SHA-256 `0799cb5151c27a1e0476519a3b2919ccd5e6b2503160e70da5c21a8eb500be8e`. [Successful full correctness run 39145303484](https://github.com/grantcharlie1000-tech/Bowser/actions/runs/39145303484), artifact 12143481419, ZIP SHA-256 `58ae1c032639a19b9e44245a3cc9bde0d57feba0d23a38325028de5a9c5e8678`. New harness identity is recorded separately; product/build inputs must remain byte-identical in source. The ordinary sidebar correctness pass and prior input hold do not validate this new bar route until the fixed Windows pilot actually succeeds.

The same package corrects only the optional sidebar disk postprocessor's acceptance of finite nonnegative integer-valued PowerShell double byte sums (≤2^53), with no native rerun or change to archived RAM/proxy inference. Invalid/fractional/unknown counts stay unavailable.

Initial harness run [39150710997](https://github.com/grantcharlie1000-tech/Bowser/actions/runs/39150710997) stopped in the disposable atomic-file contract before input download or any native sample: PowerShell converted the null backup-path argument to an empty string. The correction passes the explicit managed-null string sentinel in both real fixture update and its contract. Product inputs, fixed four-row plan and atomic-replacement semantics are unchanged; the failed contract is retained.
