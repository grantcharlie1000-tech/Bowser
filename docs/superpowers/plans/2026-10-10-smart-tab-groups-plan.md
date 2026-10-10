# Smart Tab Groups Implementation Plan

## Scope

Implement the approved design in small changes. Preserve unrelated password/security changes already present in the worktree. Do not commit.

## Steps

1. **Domain key and preferences**
   - Add a small `tab_groups` module for HTTP(S) committed-URL grouping keys using `url::Url` host canonicalization and the static `psl` crate's built-in Public Suffix List.
   - Return no key for IP literals or non-web schemes. Cover IDN/punycode, trailing dots, private suffixes, ports, single-label hosts, and public suffixes in unit tests.
   - Add versioned per-domain Auto/Always/Never and collapsed preferences with strict parsing, size/entry limits, one startup read, and atomic save only on explicit preference changes. Use local application settings storage; do not include URLs, paths, or credentials.

2. **Tab model and app integration**
   - Add pinned metadata to `Tab` and synchronized `AppTab` state, with `TabList` operations for pin/unpin that maintain a pinned prefix and preserve active `TabId` and relative order within partitions.
   - Add pure group derivation from committed tab URLs and preferences. Automatic grouping includes unpinned tabs only, does not reorder the model, and supports non-contiguous members.
   - Connect committed address updates, selection, close, add, and existing reorder methods so both tab collections remain aligned. Selecting a hidden member expands its stored domain group before selection; the active member remains visible if a collapse action targets its group.

3. **Tab strip geometry, rendering, and commands**
   - Extend the existing chrome snapshot/layout path with visible tab/group slots and a model-tab identity mapping; preserve existing DPI, overflow, active visibility, and animation behavior.
   - Keep expanded tabs as individual glass islands. Render a compact frosted group affordance/count, collapse state, and a restrained common accent. Add a group toggle hit target and a tab context menu with Pin/Unpin and Auto/Always/Never for the tab's domain.
   - Ensure pin/unpin moves only the acted-on tab to its partition, never creates a WebView, and does not alter active state. Test keyboard selection and drag/reorder against model identities after slot mapping.

4. **Regression verification**
   - Add focused model, grouping, persistence, layout, and interaction tests, including non-contiguous groups, pinning, active-tab visibility, selecting collapsed members, overflow hit testing, and corrupt/oversized settings fallback.
   - Run focused Rust tests, then `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`, and debug/release builds. Capture any unrelated/pre-existing failures separately.
   - If the Windows UI runner is available, manually exercise two domains, collapsed and expanded states, pin/unpin, reorder, close, and active-tab retention. Report unavailable UI checks rather than inferring success.
   - Compare a small before/after idle memory and CPU sample only if the existing local harness can produce directly comparable measurements; do not present a noisy sample as a performance claim.

## Review boundaries

- The `psl` crate's embedded static list is accepted as one narrowly scoped dependency for correct multi-label and private-suffix grouping. No network refresh or runtime polling is added.
- Existing tab order remains authoritative. Grouping only changes visible strip slots; explicit Pin/Unpin is the sole new action that moves a tab into the pinned prefix.
- Persistence stores grouping preferences only. It does not restore tabs or sessions.
