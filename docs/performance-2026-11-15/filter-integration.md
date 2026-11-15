# Parent request-filter integration checkpoint

The reviewed incremental `filter-plan.patch` is integrated locally. Its SHA-256 is `4A09409DA2B3D1B63D47B74CA75870D96FC5F32A00E2178E182E896CE917A99F`. The production change only borrows the registered filter list while computing owned changes, ending that borrow before any COM registration call. No persistent cache, rule policy, registration context or controller lifetime is changed.

The parent combined run passed **25 tests, zero failures**, Cargo exit 0: three new real-reconciliation tests and 22 existing registration/matcher/private/generation tests. The actual no-change reconciliation recorded one allocation call. The child's baseline run recorded 66 allocation calls and the intended failing bound. This establishes less allocation work for that operation, not measured task time, retained/process RAM, native COM performance, or the per-area 50% target.

The new test-only System-forwarding allocator uses thread-local counting only around the operation. It affects the library test binary, not shipping builds. Timed tests under that allocator cannot supply application acceptance evidence.

The exact parent command and full results are in `C:\Users\charlie\.t3\artifacts\bowser-performance-20261115\parent-filter-validation.log`. It ran with locked offline dependencies, one build job and one test thread in the shared build mutex. An initial direct PowerShell script invocation was blocked by execution policy before Cargo ran; the completed run used a process-scoped policy for the locally authored helper without changing system/user policy.

Existing partial-registration/removal failure reconciliation limits are documented in `handoffs/filter-plan-handoff.md` and remain outside this narrow copy reduction. Native worker-source coverage, shipping-feature/full-suite checks, runtime stability and paired performance/resource checks remain pending. There is no council verdict or exhausted-savings claim.
