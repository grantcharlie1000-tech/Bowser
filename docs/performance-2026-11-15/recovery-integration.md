# Parent recovery integration checkpoint

The DIB and engine-recovery packages have been reviewed and integrated locally without commits. This is unfinished integration, not a stability, visual, resource or performance acceptance claim.

The parent identified two avoidable losses in the initial process-recovery package. A temporary renderer-unresponsive event closed the live controller, discarding a document that might recover. A renderer exit also closed a controller that can remain viable, losing its back/forward history unnecessarily. The parent now retains the document on unresponsiveness and retains the controller after renderer exit for explicit GET retry. Browser-process exit still requires recreating the dead WebViews. Unknown submitted requests are never automatically replayed.

Parent regression evidence under the shared Cargo build/test mutex:

- `parent-unresponsive-red.log`: one intended behavioral failure, Cargo exit 101; the returned package closed a live unresponsive document.
- `parent-recovery-green.log`: 14 passed, one failed. The remaining assertion incorrectly assumed the existing `test_show_page` helper established `page_usable`; it only sets URL/controller state. The test setup now explicitly establishes usable content. No production change was made for that test-precondition correction.
- `parent-renderer-retention-red.log`: one intended behavioral failure, Cargo exit 101; the returned package closed the viable controller after renderer exit.
- `parent-recovery-green-final.log`: all 16 focused combined regressions passed, Cargo exit 0. This includes both parent state-preservation regressions.

Logs are in `C:\Users\charlie\.t3\artifacts\bowser-performance-20261115\`. Commands use `cargo +stable test --locked --offline --jobs 1 --lib <filter> -- --test-threads=1` and the completed engine worker's external target directory, with no concurrent writer to that target.

Before the start-cache package was integrated, the combined default-feature library suite passed: **2972 passed, 42 ignored, zero failed**, Cargo exit 0. Full raw log: `parent-library-suite.log`; duration 96.45 seconds is suite execution time, not an application performance result. The existing embedded timing assertions were included by this broad suite command; none supplies evidence for a user-facing speed claim. Shipping-feature and native GUI checks remain outstanding.

The bounded native-window package is now integrated: same-size allocation/layered-upload retries, opaque emergency native presentation and Close/Retry controls, continued retained native painting, revocable chrome lifetimes and readiness deadlines. The combined shipping-feature library suite now passes 3017 tests with 42 ignored. These are test results, not live opacity/close/recovery proof. Safe full HTML-chrome restoration after a new shared environment remains in progress because it needs a document/state application acknowledgement; the current lifetime native fallback remains an acceptance concern. Parent owns final source review, the final combined suite/release checks and isolated native GUI fault injection. The original stable-reference reproduction is separate evidence, not candidate proof.
