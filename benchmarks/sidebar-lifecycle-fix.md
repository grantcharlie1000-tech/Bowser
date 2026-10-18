# Sidebar creation lifecycle experiment

This is an isolated performance/correctness experiment, not a merge/release or an accepted memory saving. The earlier fast-input/focus acceptance blocker is unchanged.

## Preserved baseline

Run [39136883545](https://github.com/grantcharlie1000-tech/Bowser/actions/runs/39136883545), harness `ce7d73e4`, artifact `12139739451` (SHA-256 `82e5fb2a24f621106ea5f220ea6dad0e7f90a63a0f9666bb3a1832596871ce62`) completed all 16 fixed baseline rows. There were 32 executable identity checks, 16 owned-tree drains, 96 eligible resource phases and 288 native snapshots with zero coverage errors/raced exits.

Exact immediate pre-fix executable: source `8036b86d109e6aec7a39808589655a43ee34b85d`, tree `2a36d4c7af8ee00c27f950151f3ace8152fb72ed`, 7,889,408 bytes, SHA-256 `e42a1005193c54b08fba8c6f65f88757900f6bc400d4c350b0b386453406fa66`. Original build artifact `12137628254`, archive SHA-256 `c5fd895e5b4b12768736363e43b1295a4ad1f23b65284ac087cb06c64e6b668e`. It retains the existing filter minimizer. The archived minus-minimizer interaction reference is NOT this experiment's baseline.

Every settled logging-ON row created A once and B twice, replacing one B. Rapid A/B/A rows made five controller requests and three replacements. These are redundant work observations. Explicit Close counts are correctness evidence, not proof of recovered memory. The approximately 29 MiB first-B stage increase cannot be assigned to the duplicate alone. Hidden legitimate A/B pages are intentionally retained; closed-panel private commit is not itself leakage.

## Bounded product change

- Reserve environment/per-app creation generations before native dispatch; repeated callbacks/open/focus coalesce while retaining latest active app/bounds.
- Invalidate before remove, clear or shutdown. Late success/failure cannot install or update a removed/re-added app registration. Hidden legitimate pages stay live.
- Validate the current request before configure, directly before initial Navigate and before install. Apply current bounds/visibility outside state borrows, checking real state changes with at most four catch-up applications. A retired or continuously changing uninstalled controller is closed, not installed stale.
- Remove handlers and Close obsolete/replaced controllers outside RefCell borrows, including partial configure failure. Existing security decisions and download progress tracking continue; retired UI deliveries/new download plans cannot act on a new registration.
- Retry reloads a completed controller but recreates a missing one, coalescing any already-pending request. A stale outer HRESULT cannot overwrite a synchronous completion/reentrant replacement.

## Clear-data boundary

Per-profile retirement tracking is independent of panel/host/factory lifetime. Clearing first blocks new native work and invalidates generations. Filesystem deletion requires both callback retirement and affirmative BrowserProcessExited evidence for every identified browser session; completed request obligations are compacted by PID, not retained per attempt. An unidentified failed request, missing registration/release or environment-only uncertain session remains a retriable error and may require restart/recovery. A missing directory is NOT success while native work is unresolved.

No UI-thread wait, automatic later erase, async-clear UI, storage/profile merge, security weakening, focus repair, or form/session discard is introduced. Subscriptions are detached outside borrows at safe existing boundaries and idempotently before CoUninitialize, including error returns.

## First validation job, not a gain matrix

The actual candidate is rebuilt with the pinned Rust 1.99.1 toolchain and HTML-default feature. Both Rust suites, Clippy, UI reproducibility/type/lint and actual Edge module/CSP checks gate execution.

1. Existing ordinary app route: two fixed fresh/reused rows, one guarded physical fixture-heading activation click and four separately guarded Toggle actions A/B/A/A-close. Require exact fixture/native/HTML activation, original binary pins/drains, one A and one B controller, zero replacements and paired successful Closes. No resource or latency statistics.
2. LAST in the job: one ignored cfg(test) real-engine integration probe in a new isolated temporary profile, unique loopback fixture, bounded message pump. Exercises duplicate A, real pending B remove/re-add, A/B/A/latest bounds/hide, stale creation rejection, live-clear refusal, passive release observation followed by one explicit clear retry, and pending-environment shutdown. Intended pending races must actually be pending or the test fails as unexercised. It does not prove an already-queued old DOM/title callback was delivered. Partial failure evidence remains incomplete. The deliberate unknown shutdown profile is not erased and may leave helpers, so no performance matrix follows in this runner.

Only after correctness passes: run the candidate and immutable 8036 on a new matched runner, at least ten alternating AB/BA pairs EACH fresh/reused, identical logging/ordinary settings/fixture/activation/Toggle/six-phase resource workload. Primary resource endpoint is whole-tree native private commit after the third fixed closed cycle; first-B-ready and first/warm visible-ready action proxies are separate reported endpoints/regression checks. Include the separate WebApps process tree, all snapshot/identity coverage, all outliers/failures and logging costs. No source-byte-to-RAM inference, overhead subtraction or automatic 20% claim.
