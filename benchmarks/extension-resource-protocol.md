# Extension startup/resources and input correctness

Protocol2 changes only the diagnostic harness, not browser behavior. The executable remains a release build launched with --dev (suppresses the production updater) and extension tracing in BOTH arms. This is not a clean production performance audit or an optimization result.

Each fixed trial uses zero then four consented signed catalogue extensions. Existing durable ownership-count and genuine native Add/Enable-completion/no-observed-disable checks remain mandatory and are rechecked at the memory snapshot. The count comes from the durable signing-ID map, not an independent fresh native IsEnabled inventory. No missing extension is accepted as a four-extension observation.

Only trial1 starts from a fresh prepared profile. Later startup rows inherit prior navigation/autocomplete and extension-storage conditioning; they are explicitly labeled warm/conditioned, not pristine startups.

Startup/first HTTP readiness and K32GetProcessMemoryInfo PrivateUsage/WorkingSetSize for the owned whole process tree are captured BEFORE omnibox UIA/input conditioning. Summed working sets may double-count shared pages and are not PSS. A report retains every attempted row, its exact failed phase, and separately marks primary-valid and navigation-valid observations. Only trials containing valid zero AND four rows count as valid pairs. Failure still fails the workflow; a partial report is not a green result.

The following separate mandatory navigation-correctness lane uses real OS input: Ctrl+L, exact owned/focused unique address edit, literal keys at least30ms apart PLUS UIA readbacks, neutral modifier checks, exact final Value before one Enter, and the existing independent unique loopback HTTP acknowledgement. No SetValue, correction, retyping, repaired modifiers or adaptive retry. Failed/missing/ambiguous focus or mismatched Value stops input. It exercises autocomplete, but this deliberately slower observer-conditioned workload is NOT browser-only navigation latency and MUST NOT be silently compared with earlier burst-SendKeys navigation timings. Both zero/four arms execute exactly the same workload.

Captured Evergreen WebView2 process versions must be present/unique within each row and match the first observed runtime version across all compared rows. Drift refuses comparison and stops further measurements; this is a version-consistency check, not freezing or hashing the independently installed Evergreen runtime.

Prelaunch Bowser executable hashing warms its executable cache, so even trial1 is not a pristine cold-disk startup. Synchronous UIA provider calls are not hard-bounded by the surrounding polling deadlines; workflow timeouts remain outer containment, not a guaranteed per-call interruption.

The Bowser executable hash is checked before and after each attempted row; a mismatch/read error invalidates the current row and stops subsequent measurements. Owned-process cleanup errors are isolated until the attempted-row report is serialized and still fail the workflow; they cannot erase failed attempts. Synthetic fixtures/profiles/packages are not uploaded. Input traces contain lengths/focus/selection availability rather than the typed second-navigation URL content; process-identity records still include the synthetic first-launch command line.

Earlier failed reports remain evidence, not excluded outliers. For example, Store run39105231410 failed its durable ownership-count gate and run39106440296 failed second-page readiness under the old burst-input workload. Neither supplies a completed paired result. The new workload does not explain away the separate ownership-count failure.
