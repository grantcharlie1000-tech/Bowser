# Actual App native and host menu regression lane

Status: local source checkpoint only. Windows compilation, PowerShell/C# compilation, native execution, and the mandatory independent source audit are pending. This is not a runtime pass, compatibility claim, or activation recommendation. `experimental-engine-context-menu` remains absent from shipping defaults.

This separate lane does not modify the existing exact extension fixture test or its PowerShell helper. The new exact ignored test is:

`engine::app_host_menu_probe::live::live_probe_real_app_host_context_menu`

It must run by itself with `--exact --ignored --nocapture --test-threads=1`, the existing opt-in experimental feature, and `BOWSER_APP_HOST_MENU_PROBE_CONSENT=disposable-app-host-menu-synthetic-clipboard`. The existing native-safety workflow job contains the exact invocation without adding jobs, changing permissions, or increasing its time limit.

## Required real route

- `App::run`, its ordinary `BowserNativeWindow`, exact COM-owned source and negative views, and unchanged successful DocumentGate proofs.
- Empty bounded Bowser records and ID-map files, not an empty native extension inventory. Unknown runtime-native entries are never removed or ignored. The unchanged production coordinator retains and compares the complete native inventory.
- A separately armed, test-only observer captures the canonical current args/item/collection identities, full bounded tree, exact paths/kinds/current Names/IDs/states, and SHA-256 fingerprints of complete rendered UTF-16LE text. Host roles come only from the actual created item identities after successful production insertion. Labels and ID ranges confer no authority.
- The external helper recursively checks the entire HMENU tree, with 256-node/depth-six bounds, exact styles/state/SHA-256 text fingerprints, and positive unique leaf IDs. Selection follows the exact witness path, never a partial tree or global menu search.
- Submenus open by an actual OS cursor hover on the exact anchored parent item. Child HMENUs must equal GetSubMenu and identify the complete exact-owner/thread popup chain. Fresh process, foreground, neutral-key, full-tree, and hit proofs precede the one physical leaf click. No retries, fallback, focus rescue, handler invocation, command messages, accessibility Invoke, or extension API synthesis exist.
- Only the unchanged production coordinator may set SelectedCommandId. Every mode requires its exact current request's matching completed receipt, one successful native completion, and explicit menu cleanup. Selected modes require exactly one successful setter; Cancel requires zero.

## Sequence and effects

1. Cancel first: matching zero-setter completion, a two-second bounded quiet interval, no field/server/tab/storage/negative-view changes.
2. Native Copy: the current engine Name `copy` and actual engine ID, one physical HMENU selection, exact generated fixture text in the safely owned clipboard, and unchanged source/negative documents.
3. Native Paste: current Name `paste`, exact generated new token in only the source textarea, one trusted input effect, and a matching loopback nonce acknowledgment.
4. Insert note: one note seeded through the existing productivity/profile API, actual host-created parent and child COM identities, actual OS submenu traversal, exact source body/server nonce effect, and unchanged note storage.
5. Split last: actual host-created link item, exactly one new AppTab, actual `App::split_view` containing source and destination, two visible non-overlapping actual controller bounds, successful committed loopback URL and destination/server nonce, and unchanged negatives.

## Clipboard safety boundary

Clipboard access exists only in the external helper, which outlives a terminated Rust test. The lane requires an empty, unlocked clipboard before mutation and never reads, logs, hashes, backs up, or serializes prior contents. All writes contain generated sentinel/Paste text. Each controlled write and confirmed native Copy records its sequence number.

Before any Copy content read, the helper requires the exact matching successful production Copy completion, exclusive OpenClipboard, a changed sequence, and an actual current clipboard-owner HWND from the retained exact disposable browser process. Absent/ambiguous ownership refuses without reading. No text-only backup claim is made.

Finally, the guard opens the clipboard exclusively and restores empty only if its last confirmed owned sequence remains current. An unrelated actor's changed sequence or a locked clipboard is left untouched and fails the gate. This cleanup precedes the retained-handle failure termination attempt. The only termination target is the exact original single-test process handle with matching process creation time, image path, live native owner, PID, and STA. No browser or PID tree is killed.

## Explicit remaining gates

- Search completed transport is NOT RUN: configured engines require HTTPS. This slice does not relax HTTPS or substitute a loopback requested-URL claim.
- The integrated permission/race matrix and signed Bitwarden workflow remain critical unresolved gates.
- Feature-enabled Clippy remains a separate quality gate. Existing shipping-feature Clippy does not cover this feature, and pre-existing warnings in the preserved extension fixture are not cleaned up here.
- Windows compile/native execution and independent source audit are mandatory before any public run. A helper exit or a source-only JavaScript check is never native dispatch/effect acceptance.

Local deterministic check: `node benchmarks/scripts/validate-app-host-menu-fixture.mjs`. It checks fixture syntax, trusted-right-click accounting, generated tokens, loopback acknowledgment shape, and split href only. It does not exercise App, Windows input, COM, menus, clipboard, permissions, or extension compatibility.
